//! Twins of the tests of ilium-client/src/setup_prompt.rs. The manual replaces the startup offer dialog with
//! the Setup tab (a check-list of `block|target` rows plus a confirm dialog), so each twin keeps the original
//! behaviour: only missing features can be chosen, set-up rows are inert, focus skips them, Apply writes only
//! the chosen feature, shortcuts change nothing, and hit-testing maps a click to its own row.

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

const SURFACE: &str = "setup";

/// Global Claude target with Chatroom set up and Progress missing, as in the original fixtures.
fn host_with_progress_missing() -> FakeHost {
    let mut host = project_host();
    install(&mut host, "chatroom", CLAUDE_GLOBAL).expect("chatroom");
    host.settle();
    host
}

fn row(block: &str) -> String {
    format!("{block}|{CLAUDE_GLOBAL}")
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::missing_features_start_selected_and_existing_features_cannot_toggle
#[test]
fn port_sp_01_missing_features_start_focused_and_set_up_rows_cannot_be_activated() {
    let mut host = host_with_progress_missing();
    assert_eq!(host.focused_row_id(SURFACE).as_deref(), Some(row("progress").as_str()));
    let chatroom_before = get(&mut host, claude_file());
    host.press_row(SURFACE, &row("chatroom"));
    assert!(host.render_popup(80).trim().is_empty(), "a set-up row opens no dialog");
    assert_eq!(get(&mut host, claude_file()), chatroom_before);
    host.press_row(SURFACE, &row("progress"));
    assert!(host.render_popup(80).contains("Install Progress block"));
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::focus_skips_already_configured_feature_rows
#[test]
fn port_sp_02_focus_skips_already_configured_feature_rows() {
    let mut host = host_with_progress_missing();
    for key in ["up", "down", "up", "down", "down", "up"] {
        host.key_on_tab(SURFACE, key);
        let focused = host.focused_row_id(SURFACE).expect("a row stays focused");
        assert_ne!(focused, row("chatroom"), "focus landed on a set-up row after {key}");
    }
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::unavailable_mouse_feature_row_is_inert
#[test]
fn port_sp_03_unavailable_mouse_feature_row_is_inert() {
    let mut host = host_with_progress_missing();
    let before = get(&mut host, claude_file());
    let screen = host.render_kernel_tab(SURFACE, 100, 30);
    let rendered = screen.lines();
    let line = rendered
        .iter()
        .position(|line| line.contains("Chatroom") && line.contains("Set up by Ilium"))
        .expect("chatroom row for the global Claude file");
    host.click_kernel_tab(SURFACE, 2, u16::try_from(line).expect("row index"));
    assert!(host.render_popup(80).trim().is_empty());
    assert_eq!(host.focused_row_id(SURFACE).as_deref(), Some(row("progress").as_str()));
    assert_eq!(get(&mut host, claude_file()), before);
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::apply_reports_only_selected_missing_features
#[test]
fn port_sp_04_apply_writes_only_the_feature_that_was_confirmed() {
    let mut host = project_host();
    host.press_row(SURFACE, &row("progress"));
    host.press_popup_button("Install");
    host.settle();
    let text = get(&mut host, claude_file()).expect("file written");
    assert!(text.contains("ilium-agent-feature: progress"));
    assert!(!text.contains("ilium-agent-feature: chatroom"), "the unconfirmed feature is not written");
    assert_eq!(status_ok(&mut host, "chatroom", CLAUDE_GLOBAL), "not-set-up");
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::keyboard_shortcuts_return_non_mutating_decisions
#[test]
fn port_sp_05_keyboard_shortcuts_close_the_dialog_without_mutating() {
    let mut host = project_host();
    for key in ["escape", "n"] {
        host.press_row(SURFACE, &row("progress"));
        host.key_on_popup(key);
        host.settle();
        assert!(host.render_popup(80).trim().is_empty(), "{key} closes the dialog");
        assert_eq!(get(&mut host, claude_file()), None, "{key} writes nothing");
    }
}

/// REQ: PORT:ilium-client/src/setup_prompt.rs::hit_test_tracks_each_visible_control_row
#[test]
fn port_sp_06_hit_test_tracks_each_visible_control_row() {
    let mut host = project_host();
    let screen = host.render_kernel_tab(SURFACE, 100, 30);
    let rendered = screen.lines();
    let lines: Vec<&str> = rendered.iter().map(String::as_str).collect();
    for (block, label) in [("chatroom", "Chatroom"), ("progress", "Progress")] {
        let index = lines.iter().position(|line| line.contains(label)).expect("row is visible");
        host.click_kernel_tab(SURFACE, 2, u16::try_from(index).expect("row index"));
        let dialog = host.render_popup(80);
        assert!(dialog.contains(&format!("Install {label} block")), "{block}\n{dialog}");
        host.press_popup_button("Cancel");
    }
    host.click_kernel_tab(SURFACE, 0, 0);
    assert!(host.render_popup(80).trim().is_empty(), "a click outside every row hits nothing");
}
