//! The kernel recovery page (settings-screens.md, core-plugin-settings.md,
//! user-troubleshooting.md): USR-032, USR-160, USR-161 (recovery side).

use crate::support::*;

// ---- USR-032 ----

/// REQ: USR-032
#[test]
fn usr_032_recovery_page_opens_while_the_settings_plugin_is_off_and_settings_can_return() {
    let mut rig = ScreensRig::new(60, 30);
    rig.set_plugin_state_off("ilium.core.settings");
    rig.press_prefix_then(':');
    assert!(rig.screen_text().contains("Recovery"), "{}", rig.screen_text());
    rig.press_key('e');
    rig.select_plugin("ilium.core.settings");
    rig.press_key('e');
    assert!(rig.plugin_is_running("ilium.core.settings"), "re-enabled from the recovery page");
}

/// REQ: USR-032
#[test]
fn usr_032_recovery_page_opens_while_the_settings_plugin_has_failed() {
    let mut rig = ScreensRig::new(60, 30);
    rig.fail_plugin_three_times("ilium.core.settings");
    rig.press_prefix_then(':');
    assert!(rig.screen_text().contains("Recovery"));
    assert!(has_row_with(&rig.screen_text(), "ilium.core.settings"));
}

/// REQ: USR-032
#[test]
fn usr_032_a_plugin_cannot_replace_the_recovery_page() {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_plugin_that_claims_recovery_page("com.example.fake");
    rig.press_prefix_then(':');
    let screen = rig.screen_text();
    assert!(screen.contains("Recovery"));
    assert!(!screen.contains("com.example.fake page"), "{screen}");
    assert!(rig.last_install_error().is_some() || rig.recovery_page_owner() == "kernel");
}

// ---- USR-160 ----

/// REQ: USR-160
#[test]
fn usr_160_the_page_shows_every_action_with_its_key() {
    let mut rig = ScreensRig::new(60, 30);
    rig.open_recovery_page();
    let screen = rig.screen_text();
    for (key_and_label, action) in [
        ("e enable", "enable"),
        ("d disable", "disable"),
        ("r reset failures", "reset failures"),
        ("u uninstall", "uninstall"),
        ("f forget settings", "forget settings"),
        ("s safe mode", "safe mode"),
        ("o open config file", "open config file"),
        ("q close", "close"),
    ] {
        assert!(screen.contains(key_and_label), "{action}: {screen}");
    }
}

/// REQ: USR-160
#[test]
fn usr_160_ctrl_b_colon_opens_the_page_when_the_settings_plugin_is_off() {
    let mut rig = ScreensRig::new(60, 30);
    rig.set_plugin_state_off("ilium.core.settings");
    rig.press_prefix_then(':');
    assert!(rig.screen_text().contains("Recovery"));
    rig.press_key('q');
    assert!(!rig.screen_text().contains("Recovery"));
}

/// REQ: USR-160
#[test]
fn usr_160_each_key_runs_its_action_on_the_selected_plugin() {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_plugin("com.example.target", "T2");
    rig.open_recovery_page();
    rig.select_plugin("com.example.target");
    rig.press_key('d');
    assert_eq!(rig.plugin_state_word("com.example.target"), "Off");
    rig.press_key('e');
    assert_eq!(rig.plugin_state_word("com.example.target"), "On");
    rig.put_plugin_in_state("com.example.target", RowState::Failed);
    rig.press_key('r');
    assert_eq!(rig.failure_count("com.example.target"), 0);
    assert_eq!(rig.plugin_state_word("com.example.target"), "Failed", "reset clears the count without activating (LIF-272)");
}

/// REQ: USR-160
#[test]
fn usr_160_safe_mode_and_open_config_file_are_reported_to_the_host() {
    let mut rig = ScreensRig::new(60, 30);
    rig.open_recovery_page();
    rig.press_key('o');
    assert_eq!(rig.requested_editor_paths().len(), 1);
    assert!(rig.requested_editor_paths()[0].ends_with("config.toml"));
    rig.press_key('s');
    assert!(rig.restart_requested_in_safe_mode());
}

/// REQ: USR-160
#[test]
fn usr_160_failed_plugins_are_listed_before_enabled_ones() {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_plugin("com.example.a", "T2");
    rig.install_plugin("com.example.z", "T2");
    rig.put_plugin_in_state("com.example.z", RowState::Failed);
    rig.open_recovery_page();
    let screen = rig.screen_text();
    let failed_at = screen.find("com.example.z").expect("failed row");
    let enabled_at = screen.find("com.example.a").expect("enabled row");
    assert!(failed_at < enabled_at);
}

// ---- USR-161 (recovery surface) ----

/// REQ: USR-161
#[test]
fn usr_161_recovery_page_uses_the_failed_word_for_a_failed_plugin() {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_plugin("com.example.z", "T2");
    rig.put_plugin_in_state("com.example.z", RowState::Failed);
    rig.open_recovery_page();
    assert!(row_with(&rig.screen_text(), "com.example.z").contains("Failed"));
}
