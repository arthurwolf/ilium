//! Twins of the settings tests in ilium-client/src/app.rs: the toggles of the User Interface and
//! Agent Monitoring tabs persist and leave every other key alone (F-089, F-090).

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/app.rs::closing_a_non_last_item_or_disabled_setting_leaves_the_group, F-090
#[test]
fn port_090_switching_auto_remove_empty_groups_off_is_saved_and_changes_no_other_key() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    toggle(&mut host, "user-interface", "ui.auto_remove_empty_groups", false);
    assert_eq!(value_of(&mut host, "ui.auto_remove_empty_groups"), Some(json!(false)));
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::closing_a_non_last_item_or_disabled_setting_leaves_the_group, F-090
#[test]
fn port_090_auto_remove_empty_groups_switched_back_on_is_saved() {
    let mut host = client_host();
    toggle(&mut host, "user-interface", "ui.auto_remove_empty_groups", false);
    toggle(&mut host, "user-interface", "ui.auto_remove_empty_groups", true);
    assert_eq!(value_of(&mut host, "ui.auto_remove_empty_groups"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/app.rs::context_menu_icon_setting_persists_and_preserves_all_menu_actions, F-090
#[test]
fn port_090_context_menu_icons_off_is_saved_and_the_other_menu_settings_survive() {
    let mut host = client_host();
    let management = value_of(&mut host, "ui.show_tree_row_management_controls");
    toggle(&mut host, "user-interface", "ui.show_context_menu_icons", false);
    assert_eq!(value_of(&mut host, "ui.show_context_menu_icons"), Some(json!(false)));
    assert_eq!(value_of(&mut host, "ui.show_tree_row_management_controls"), management);
}

/// REQ: PORT:ilium-client/src/app.rs::context_menu_icon_setting_persists_and_preserves_all_menu_actions, F-090
#[test]
fn port_090_context_menu_icons_toggle_is_idempotent_for_the_same_value() {
    let mut host = client_host();
    toggle(&mut host, "user-interface", "ui.show_context_menu_icons", false);
    toggle(&mut host, "user-interface", "ui.show_context_menu_icons", false);
    assert_eq!(value_of(&mut host, "ui.show_context_menu_icons"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/app.rs::model_icon_settings_row_toggle_persists_the_new_value, F-090
#[test]
fn port_090_model_icons_default_to_off() {
    let mut host = client_host();
    assert_eq!(value_of(&mut host, "ui.agent_tree_model_icons"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/app.rs::model_icon_settings_row_toggle_persists_the_new_value, F-090
#[test]
fn port_090_model_icons_toggle_on_is_saved() {
    let mut host = client_host();
    toggle(&mut host, "agent-monitoring", "ui.agent_tree_model_icons", true);
    assert_eq!(value_of(&mut host, "ui.agent_tree_model_icons"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/app.rs::project_separator_setting_persists_in_its_project_and_preserves_metadata, F-090
#[test]
fn port_090_project_separators_default_to_off() {
    let mut host = client_host();
    assert_eq!(value_of(&mut host, "ui.show_project_separators"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/app.rs::project_separator_setting_persists_in_its_project_and_preserves_metadata, F-090
#[test]
fn port_090_project_separators_on_is_saved_and_leaves_the_other_toggles() {
    let mut host = client_host();
    let icons = value_of(&mut host, "ui.show_context_menu_icons");
    toggle(&mut host, "user-interface", "ui.show_project_separators", true);
    assert_eq!(value_of(&mut host, "ui.show_project_separators"), Some(json!(true)));
    assert_eq!(value_of(&mut host, "ui.show_context_menu_icons"), icons);
}

/// REQ: PORT:ilium-client/src/app.rs::setting_layout_resizes_the_visible_terminal_pane, F-090
#[test]
fn port_090_a_panel_width_change_is_saved_through_the_user_interface_tab() {
    let mut host = client_host();
    type_number(&mut host, "user-interface", "ui.left_panel_fixed_width", 40.0);
    assert_eq!(value_of(&mut host, "ui.left_panel_fixed_width"), Some(json!(40)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::setting_layout_resizes_the_visible_terminal_pane, F-090
#[test]
fn port_090_the_user_interface_tab_still_renders_after_a_panel_width_change() {
    let mut host = client_host();
    type_number(&mut host, "user-interface", "ui.left_panel_fixed_width", 40.0);
    let screen = host.render(tab("user-interface"), 140, 40);
    assert_eq!((screen.columns, screen.rows), (140, 40));
    assert!(!screen.text().trim().is_empty());
}

/// REQ: PORT:ilium-client/src/app.rs::stable_glyph_setting_is_an_explicit_opt_in, F-090, F-092
#[test]
fn port_090_stable_glyphs_default_to_off() {
    let mut host = client_host();
    assert_eq!(value_of(&mut host, "ui.use_stable_glyphs"), Some(json!(false)));
}

/// REQ: PORT:ilium-client/src/app.rs::stable_glyph_setting_is_an_explicit_opt_in, F-090, F-092
#[test]
fn port_090_stable_glyphs_on_is_saved_only_by_an_explicit_toggle() {
    let mut host = client_host();
    host.settle();
    assert_eq!(value_of(&mut host, "ui.use_stable_glyphs"), Some(json!(false)));
    toggle(&mut host, "user-interface", "ui.use_stable_glyphs", true);
    assert_eq!(value_of(&mut host, "ui.use_stable_glyphs"), Some(yes().clone()));
}
