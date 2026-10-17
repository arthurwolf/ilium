//! Footer indicators, toasts and persistent notices: USR-087, USR-088 (render), USR-130,
//! USR-211, USR-212 (notice text).

use crate::support::*;

// ---- USR-087 ----

/// REQ: USR-087
#[test]
fn usr_087_footer_shows_a_clipboard_indicator_for_three_seconds() {
    let mut rig = ScreensRig::new(80, 24);
    rig.plugin_reads_clipboard("com.example.clip");
    assert!(
        rig.footer_text().to_lowercase().contains("clipboard"),
        "{}",
        rig.footer_text()
    );
    rig.advance_ms(2_999);
    assert!(
        rig.footer_text().to_lowercase().contains("clipboard"),
        "n-1"
    );
    rig.advance_ms(2);
    assert!(
        !rig.footer_text().to_lowercase().contains("clipboard"),
        "n+1"
    );
}

/// REQ: USR-087
#[test]
fn usr_087_a_clipboard_write_names_the_plugin_in_the_indicator() {
    let mut rig = ScreensRig::new(80, 24);
    rig.plugin_writes_clipboard("com.example.clip");
    assert!(rig.footer_text().contains("com.example.clip"));
}

/// REQ: USR-087
#[test]
fn usr_087_a_second_use_restarts_the_three_seconds() {
    let mut rig = ScreensRig::new(80, 24);
    rig.plugin_reads_clipboard("com.example.clip");
    rig.advance_ms(2_000);
    rig.plugin_reads_clipboard("com.example.clip");
    rig.advance_ms(2_000);
    assert!(rig.footer_text().to_lowercase().contains("clipboard"));
}

// ---- USR-088 (render) ----

/// REQ: USR-088
#[test]
fn usr_088_ten_toasts_at_once_draw_three() {
    let mut rig = ScreensRig::new(80, 24);
    for index in 0..10 {
        rig.plugin_toasts("com.example.toast", &format!("toast number {index}"));
    }
    assert_eq!(rig.screen_text().matches("toast number").count(), 3);
}

/// REQ: USR-088
#[test]
fn usr_088_the_input_lock_releases_after_two_seconds_on_screen() {
    let mut rig = ScreensRig::new(80, 24);
    rig.plugin_locks_input("com.example.lock");
    rig.press_key('x');
    assert!(!rig.pane_received_input('x'), "locked");
    rig.advance_ms(2_001);
    rig.press_key('y');
    assert!(rig.pane_received_input('y'), "released");
}

// ---- USR-130 ----

/// REQ: USR-130
#[test]
fn usr_130_footer_and_settings_row_show_the_same_n14_text() {
    let mut rig = ScreensRig::new(48, 30);
    rig.install_plugin_that_traps_on_activate("com.example.cursor", "Cursor keeper");
    rig.advance_ms(7_000);
    let expected = "Plugin Cursor keeper stopped after 3 failures in 10 minutes: activate trap. Turn it on again in Settings > Plugins.";
    let footer = rig.footer_text();
    let normalised: String = footer.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalised.contains(expected.trim_start_matches("Plugin "))
            || normalised.contains(expected),
        "footer: {footer}"
    );
    rig.open_plugins_tab();
    let tab: String = rig
        .screen_text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(tab.contains("stopped after 3 failures in 10 minutes: activate trap. Turn it on again in Settings > Plugins."), "tab: {tab}");
}

/// REQ: USR-130
#[test]
fn usr_130_the_notice_uses_the_danger_colour() {
    let mut rig = ScreensRig::new(48, 30);
    rig.install_plugin_that_traps_on_activate("com.example.cursor", "Cursor keeper");
    rig.advance_ms(7_000);
    let colour = rig.footer_notice_foreground().expect("a coloured notice");
    assert_eq!(colour, rig.theme_danger_colour());
}

/// REQ: USR-130
#[test]
fn usr_130_after_turning_the_plugin_on_the_notice_disappears() {
    let mut rig = ScreensRig::new(48, 30);
    rig.install_plugin_that_traps_on_activate("com.example.cursor", "Cursor keeper");
    rig.advance_ms(7_000);
    rig.open_plugins_tab();
    rig.move_selection_to("com.example.cursor");
    rig.press_space();
    assert!(!rig.footer_text().contains("stopped after 3 failures"));
}

/// REQ: USR-130
#[test]
fn usr_130_view_name_appears_for_a_render_trap() {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_plugin_that_traps_in_render("com.example.view", "Viewer", "sidebar");
    rig.advance_ms(7_000);
    let normalised: String = rig
        .footer_text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        normalised.contains("views.render trap (sidebar)"),
        "{normalised}"
    );
}

// ---- USR-211 ----

/// REQ: USR-211
#[test]
fn usr_211_a_world_writable_policy_shows_a_persistent_notice_on_every_start() {
    let mut rig = ScreensRig::new(80, 24);
    rig.write_system_policy_with_mode("version = 1\n", 0o666);
    rig.start_client();
    let first = rig.screen_text();
    assert!(first.to_lowercase().contains("policy"), "{first}");
    rig.advance_ms(600_000);
    assert!(
        rig.screen_text().to_lowercase().contains("policy"),
        "persistent, not a toast"
    );
    rig.restart_client();
    assert!(rig.screen_text().to_lowercase().contains("policy"));
}

/// REQ: USR-211
#[test]
fn usr_211_a_policy_with_mode_0644_shows_no_notice() {
    let mut rig = ScreensRig::new(80, 24);
    rig.write_system_policy_with_mode("version = 1\n", 0o644);
    rig.start_client();
    assert!(!rig.screen_text().to_lowercase().contains("policy file"));
}

// ---- USR-212 ----

/// REQ: USR-212
#[test]
fn usr_212_the_notice_names_the_line_and_the_key_of_the_first_error() {
    let mut rig = ScreensRig::new(80, 24);
    rig.write_system_policy_with_mode(
        "version = 1\n\n[packages]\nallow_thirdparty = true\n",
        0o644,
    );
    rig.start_client();
    let screen = rig.screen_text();
    assert!(screen.contains("allow_thirdparty"), "{screen}");
    assert!(screen.contains("line 4"), "{screen}");
}

/// REQ: USR-212
#[test]
fn usr_212_only_the_first_error_is_named() {
    let mut rig = ScreensRig::new(80, 24);
    rig.write_system_policy_with_mode(
        "version = 1\n[packages]\nfirst_bad = 1\nsecond_bad = 2\n",
        0o644,
    );
    rig.start_client();
    let screen = rig.screen_text();
    assert!(screen.contains("first_bad"));
    assert!(!screen.contains("second_bad"));
}
