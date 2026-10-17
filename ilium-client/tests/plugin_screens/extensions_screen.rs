//! The Extensions tab as drawn (user-extensions-tab.md): USR-050, USR-053, USR-054.

use crate::support::*;

fn rig_with_two_contributors() -> ScreensRig {
    let mut rig = ScreensRig::new(60, 30);
    rig.install_owner_with_point("com.example.owner", "Smart Copy", "After copy");
    rig.install_contributor(
        "com.example.first",
        "com.example.owner",
        "After copy",
        "strip_tokens",
        10,
    );
    rig.install_contributor(
        "com.example.second",
        "com.example.owner",
        "After copy",
        "mask_emails",
        20,
    );
    rig
}

// ---- USR-050 ----

/// REQ: USR-050
#[test]
fn usr_050_owner_view_lists_contributors_in_run_order() {
    let mut rig = rig_with_two_contributors();
    rig.open_extensions_owner_view("com.example.owner");
    let screen = rig.screen_text();
    let first_at = screen.find("strip_tokens").expect("first");
    let second_at = screen.find("mask_emails").expect("second");
    assert!(first_at < second_at);
    assert!(row_with(&screen, "strip_tokens").contains(" 1 "));
    assert!(row_with(&screen, "mask_emails").contains(" 2 "));
}

/// REQ: USR-050
#[test]
fn usr_050_moving_the_second_row_up_swaps_the_numbers_and_wins_over_priority() {
    let mut rig = rig_with_two_contributors();
    rig.open_extensions_owner_view("com.example.owner");
    rig.move_selection_to("mask_emails");
    rig.press_key_named("Up");
    let screen = rig.screen_text();
    assert!(screen.find("mask_emails").unwrap() < screen.find("strip_tokens").unwrap());
    assert!(row_with(&screen, "mask_emails").contains(" 1 "));
    assert_eq!(
        rig.run_order_of_point("com.example.owner", "After copy"),
        vec!["com.example.second", "com.example.first"]
    );
}

/// REQ: USR-050
#[test]
fn usr_050_space_switches_a_contributor_off_and_the_key_hint_line_is_shown() {
    let mut rig = rig_with_two_contributors();
    rig.open_extensions_owner_view("com.example.owner");
    rig.move_selection_to("strip_tokens");
    rig.press_space();
    assert!(row_with(&rig.screen_text(), "strip_tokens").contains("[ ]"));
    assert!(rig.screen_text().contains("Up/Down move   Space on or off"));
}

// ---- USR-053 ----

/// REQ: USR-053
#[test]
fn usr_053_contributor_view_shows_the_stop_notice_and_a_reset_button() {
    let mut rig = rig_with_two_contributors();
    rig.breach_extension("com.example.first", "com.example.owner", "After copy", 3);
    rig.open_extensions_contributor_view("com.example.first");
    let screen = rig.screen_text();
    assert!(screen.contains("stopped after 3 timeouts"), "{screen}");
    assert!(screen.contains("[Reset]"), "{screen}");
}

/// REQ: USR-053
#[test]
fn usr_053_two_breaches_show_no_notice_and_pressing_reset_clears_the_notice() {
    let mut rig = rig_with_two_contributors();
    rig.breach_extension("com.example.first", "com.example.owner", "After copy", 2);
    rig.open_extensions_contributor_view("com.example.first");
    assert!(!rig.screen_text().contains("stopped after 3 timeouts"));
    rig.breach_extension("com.example.first", "com.example.owner", "After copy", 1);
    assert!(rig.screen_text().contains("[Reset]"));
    rig.activate("[Reset]");
    assert!(!rig.screen_text().contains("stopped after 3 timeouts"));
}

/// REQ: USR-053
#[test]
fn usr_053_after_a_client_restart_the_stop_notice_is_gone() {
    let mut rig = rig_with_two_contributors();
    rig.breach_extension("com.example.first", "com.example.owner", "After copy", 3);
    rig.restart_client();
    rig.open_extensions_contributor_view("com.example.first");
    assert!(!rig.screen_text().contains("stopped after 3 timeouts"));
}

// ---- USR-054 ----

/// REQ: USR-054
#[test]
fn usr_054_missing_owner_shows_waiting_for_owner_and_returns_to_active() {
    let mut rig = rig_with_two_contributors();
    rig.remove_plugin("com.example.owner");
    rig.open_extensions_contributor_view("com.example.first");
    assert!(
        rig.screen_text().contains("waiting for com.example.owner"),
        "{}",
        rig.screen_text()
    );
    rig.install_owner_with_point("com.example.owner", "Smart Copy", "After copy");
    rig.open_extensions_contributor_view("com.example.first");
    assert!(!rig.screen_text().contains("waiting for"));
}

/// REQ: USR-054
#[test]
fn usr_054_an_owner_that_is_off_gives_the_same_label_and_the_switch_keeps_its_setting() {
    let mut rig = rig_with_two_contributors();
    rig.set_plugin_state_off("com.example.owner");
    rig.open_extensions_contributor_view("com.example.first");
    let screen = rig.screen_text();
    assert!(screen.contains("waiting for com.example.owner"));
    assert!(row_with(&screen, "strip_tokens").contains("[x]"));
}
