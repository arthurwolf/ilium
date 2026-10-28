//! The Settings > Plugins tab (core-plugin-settings.md, user-core-plugins.md,
//! user-permissions-explained.md, user-troubleshooting.md): USR-031, USR-041, USR-080, USR-161
//! to USR-164.

use crate::support::*;

// ---- USR-031 ----

/// REQ: USR-031
#[test]
fn usr_031_safe_mode_shows_the_banner_on_the_tree_and_on_the_plugins_tab() {
    let mut rig = ScreensRig::new(60, 24);
    rig.install_plugin("com.example.notes", "T2");
    rig.install_plugin("com.example.old", "T3");
    rig.start_client_safe_mode();
    let banner = "Safe mode: third-party plugins are off.";
    assert!(
        rig.screen_text().contains(banner),
        "tree screen: {}",
        rig.screen_text()
    );
    rig.open_plugins_tab();
    assert!(
        rig.screen_text().contains(banner),
        "plugins tab: {}",
        rig.screen_text()
    );
}

/// REQ: USR-031
#[test]
fn usr_031_every_third_party_row_reads_off_with_the_reason_safe_mode() {
    let mut rig = ScreensRig::new(60, 24);
    rig.install_plugin("com.example.notes", "T2");
    rig.install_plugin("com.example.old", "T3");
    rig.start_client_safe_mode();
    rig.open_plugins_tab();
    let screen = rig.screen_text();
    for id in ["com.example.notes", "com.example.old"] {
        assert!(row_with(&screen, id).contains("Off, safe mode"), "{id}");
    }
}

/// REQ: USR-031
#[test]
fn usr_031_core_rows_are_not_labelled_safe_mode_and_a_normal_start_has_no_banner() {
    let mut rig = ScreensRig::new(60, 40);
    rig.install_plugin("com.example.notes", "T2");
    rig.start_client_safe_mode();
    rig.open_plugins_tab();
    assert!(!row_with(&rig.screen_text(), "ilium.core.tree").contains("safe mode"));
    let mut normal = ScreensRig::new(60, 24);
    normal.start_client();
    assert!(!normal.screen_text().contains("Safe mode:"));
}

/// REQ: USR-031
#[test]
fn usr_031_the_banner_is_on_the_settings_screen_and_on_the_recovery_page_too() {
    let mut rig = ScreensRig::new(60, 24);
    rig.start_client_safe_mode();
    rig.open_settings();
    assert!(rig
        .screen_text()
        .contains("Safe mode: third-party plugins are off."));
    rig.open_recovery_page();
    assert!(rig
        .screen_text()
        .contains("Safe mode: third-party plugins are off."));
}

// ---- USR-041 ----

/// REQ: USR-041
#[test]
fn usr_041_space_on_a_protected_core_plugin_is_refused_with_a_reason() {
    let mut rig = ScreensRig::new(60, 40);
    rig.open_plugins_tab();
    rig.move_selection_to("ilium.core.tree");
    rig.press_space();
    let screen = rig.screen_text();
    assert!(
        row_with(&screen, "ilium.core.tree").contains("On"),
        "{screen}"
    );
    let notice = rig.last_notice().expect("a refusal text");
    assert!(!notice.is_empty());
    assert!(
        notice.to_lowercase().contains("cannot") || notice.to_lowercase().contains("always"),
        "{notice}"
    );
}

/// REQ: USR-041
#[test]
fn usr_041_space_on_a_free_core_plugin_turns_it_off() {
    let mut rig = ScreensRig::new(60, 40);
    rig.open_plugins_tab();
    rig.move_selection_to("ilium.core.kanban");
    rig.press_space();
    assert!(row_with(&rig.screen_text(), "ilium.core.kanban").contains("Off"));
    assert!(rig.last_notice().is_none());
}

/// REQ: USR-041
#[test]
fn usr_041_a_core_row_offers_no_remove_action() {
    let mut rig = ScreensRig::new(60, 40);
    rig.open_plugins_tab();
    rig.move_selection_to("ilium.core.editor");
    rig.open_details();
    let screen = rig.screen_text();
    assert!(
        !screen.contains("Uninstall") && !screen.contains("Remove"),
        "{screen}"
    );
    assert!(
        !screen.contains("Install from file")
            || !has_row_with(&screen, "ilium.core.editor Install")
    );
}

// ---- USR-080 ----

/// REQ: USR-080
#[test]
fn usr_080_capabilities_tab_lists_ten_grants_with_wording_string_risk_and_state() {
    let grants = [
        "tree.read",
        "tree.write",
        "panes.read",
        "notify.toast",
        "clipboard.write",
        "clipboard.read",
        "terminal.write",
        "fs.read:project",
        "http:api.example.com",
        "process.run:git",
    ];
    let mut rig = ScreensRig::new(100, 40);
    rig.install_plugin_with_grants("com.example.ten", &grants);
    rig.set_grant_state("com.example.ten", "tree.write", "declined");
    rig.set_grant_state("com.example.ten", "clipboard.read", "revoked");
    rig.open_capabilities_tab("com.example.ten");
    let screen = rig.screen_text();
    for grant in grants {
        let row = row_with(&screen, grant);
        let expected_high =
            ["clipboard.read", "terminal.write", "process.run:git"].contains(&grant);
        assert_eq!(row.contains("HIGH"), expected_high, "{grant}: {row}");
    }
    assert!(row_with(&screen, "tree.read").contains("granted"));
    assert!(row_with(&screen, "tree.write").contains("declined"));
    assert!(row_with(&screen, "clipboard.read").contains("revoked"));
    for grant in grants {
        assert!(rig.plain_wording(grant).len() > 3);
        assert!(
            screen.contains(&rig.plain_wording(grant)),
            "plain wording of {grant}"
        );
    }
}

/// REQ: USR-080
#[test]
fn usr_080_the_tab_shows_a_row_even_for_a_grant_the_user_never_answered() {
    let mut rig = ScreensRig::new(100, 20);
    rig.install_plugin_with_grants("com.example.one", &["tree.read"]);
    rig.open_capabilities_tab("com.example.one");
    assert!(has_row_with(&rig.screen_text(), "tree.read"));
}

/// REQ: USR-080
#[test]
fn usr_080_toggling_a_row_changes_its_state_word_at_once() {
    let mut rig = ScreensRig::new(100, 20);
    rig.install_plugin_with_grants("com.example.one", &["tree.read"]);
    rig.open_capabilities_tab("com.example.one");
    rig.move_selection_to("tree.read");
    rig.press_space();
    assert!(row_with(&rig.screen_text(), "tree.read").contains("revoked"));
}

// ---- USR-161 ----

/// REQ: USR-161
#[test]
fn usr_161_the_three_surfaces_use_the_same_row_words_for_each_state() {
    let states = [
        (RowState::On, "On"),
        (RowState::OnSlow, "On, slow"),
        (RowState::Restarting, "Restarting"),
        (RowState::Failed, "Failed"),
        (RowState::Off, "Off"),
        (
            RowState::WaitingFor("com.example.owner".into()),
            "Waiting for com.example.owner",
        ),
        (RowState::BlockedByPolicy, "Blocked by policy"),
        (
            RowState::Refused("no sandbox".into()),
            "Refused: no sandbox",
        ),
    ];
    let mut rig = ScreensRig::new(100, 60);
    for (index, (state, _)) in states.iter().enumerate() {
        rig.install_plugin(&format!("com.example.s{index}"), "T2");
        rig.put_plugin_in_state(&format!("com.example.s{index}"), state.clone());
    }
    rig.open_plugins_tab();
    let tab = rig.screen_text();
    let list = rig.cli_list_text();
    rig.open_recovery_page();
    let recovery = rig.screen_text();
    for (index, (_, word)) in states.iter().enumerate() {
        let id = format!("com.example.s{index}");
        assert!(row_with(&tab, &id).contains(word), "tab: {word}");
        assert!(row_with(&list, &id).contains(word), "list: {word}");
        if !matches!(states[index].0, RowState::On | RowState::OnSlow) {
            assert!(row_with(&recovery, &id).contains(word), "recovery: {word}");
        }
    }
}

/// REQ: USR-161
#[test]
fn usr_161_no_other_word_is_used_for_a_state() {
    let mut rig = ScreensRig::new(100, 60);
    rig.install_plugin("com.example.f", "T2");
    rig.put_plugin_in_state("com.example.f", RowState::Failed);
    rig.open_plugins_tab();
    let row = row_with(&rig.screen_text(), "com.example.f");
    for banned in ["stopped", "crashed", "broken", "disabled", "error"] {
        assert!(!row.to_lowercase().contains(banned), "{banned} in {row}");
    }
}

// ---- USR-162 ----

/// REQ: USR-162
#[test]
fn usr_162_two_installs_give_32_rows_with_tier_badges_matching_the_cli_list() {
    let mut rig = ScreensRig::new(100, 80);
    rig.install_plugin("com.example.a", "T2");
    rig.install_plugin("com.example.b", "T3");
    rig.open_plugins_tab();
    assert_eq!(rig.plugin_row_count(), 32);
    let screen = rig.screen_text();
    assert!(row_with(&screen, "com.example.a").contains("T2"));
    assert!(row_with(&screen, "com.example.b").contains("T3"));
    assert!(row_with(&screen, "ilium.core.tree").contains("T1"));
    let list = rig.cli_list_text();
    assert_eq!(
        list.lines()
            .filter(|line| line.contains("com.example.") || line.contains("ilium.core."))
            .count(),
        32
    );
}

/// REQ: USR-162
#[test]
fn usr_162_a_fresh_profile_shows_30_rows() {
    let mut rig = ScreensRig::new(100, 80);
    rig.open_plugins_tab();
    assert_eq!(rig.plugin_row_count(), 30);
}

// ---- USR-163 ----

/// REQ: USR-163
#[test]
fn usr_163_forget_settings_asks_confirmation_resets_the_value_and_keeps_grants() {
    let mut rig = ScreensRig::new(80, 30);
    rig.install_plugin_with_grants("com.example.keep", &["tree.read"]);
    rig.set_plugin_setting("com.example.keep", "density", "compact");
    let grants_before = rig.grants_file_bytes();
    rig.open_plugin_details("com.example.keep");
    assert!(rig.screen_text().contains("Forget settings"));
    rig.activate("Forget settings");
    assert!(
        rig.screen_text().to_lowercase().contains("forget"),
        "a confirmation question is shown"
    );
    assert_eq!(
        rig.plugin_setting("com.example.keep", "density").as_deref(),
        Some("compact"),
        "nothing happens before the answer"
    );
    rig.confirm_yes();
    assert_eq!(
        rig.plugin_setting("com.example.keep", "density"),
        None,
        "the default applies again"
    );
    assert_eq!(rig.grants_file_bytes(), grants_before);
}

/// REQ: USR-163
#[test]
fn usr_163_declining_the_confirmation_keeps_the_values() {
    let mut rig = ScreensRig::new(80, 30);
    rig.install_plugin("com.example.keep", "T2");
    rig.set_plugin_setting("com.example.keep", "density", "compact");
    rig.open_plugin_details("com.example.keep");
    rig.activate("Forget settings");
    rig.confirm_no();
    assert_eq!(
        rig.plugin_setting("com.example.keep", "density").as_deref(),
        Some("compact")
    );
}

/// REQ: USR-163
#[test]
fn usr_163_project_scope_values_of_the_open_project_are_forgotten_too() {
    let mut rig = ScreensRig::new(80, 30);
    rig.install_plugin("com.example.keep", "T2");
    rig.set_plugin_setting_project("com.example.keep", "density", "wide");
    rig.open_plugin_details("com.example.keep");
    rig.activate("Forget settings");
    rig.confirm_yes();
    assert_eq!(
        rig.plugin_setting_project("com.example.keep", "density"),
        None
    );
}

// ---- USR-164 ----

/// REQ: USR-164
#[test]
fn usr_164_the_footer_notice_and_the_row_use_the_same_state_words() {
    let mut rig = ScreensRig::new(100, 30);
    rig.install_plugin_that_traps_on_activate("com.example.cursor", "Cursor keeper");
    rig.advance_ms(7_000);
    let footer = rig.footer_text();
    rig.open_plugins_tab();
    let row = row_with(&rig.screen_text(), "com.example.cursor");
    assert!(row.contains("Failed"));
    assert!(
        footer.contains("Turn it on again in Settings > Plugins."),
        "{footer}"
    );
    let list = rig.cli_list_text();
    assert!(row_with(&list, "com.example.cursor").contains("Failed"));
}
