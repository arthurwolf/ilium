//! The 35 actions, their `kernel.<name>` command ids, defaults and the two complete presets as
//! the page states them (COR-011: every plugin-owned feature keeps its keys and defaults).

use std::collections::BTreeSet;

use proptest::prelude::*;

use crate::common::*;

const EXPECTED_NAMES: [&str; 35] = [
    "new_terminal",
    "new_agent_worktree",
    "new_editor",
    "new_board",
    "close_pane",
    "new_group",
    "new_split_view",
    "new_folder",
    "rename",
    "toggle_move",
    "focus_tree",
    "focus_pane",
    "focus_next_pane",
    "focus_previous_pane",
    "focus_pane_left",
    "focus_pane_right",
    "focus_pane_up",
    "focus_pane_down",
    "cycle_next_in_group",
    "cycle_previous_in_group",
    "jump_next_group",
    "jump_previous_group",
    "scrollback_up",
    "scrollback_down",
    "save",
    "run_command",
    "search",
    "Help",
    "detach",
    "quit",
    "toggle_editor_view_mode",
    "toggle_line_numbers",
    "toggle_minimap",
    "toggle_autosave",
    "settings",
];

fn bindable(key: &str) -> bool {
    matches!(key, "Left" | "Right" | "Up" | "Down" | "PgUp" | "PgDn")
        || (key.chars().count() == 1
            && key
                .chars()
                .all(|character| character.is_ascii_graphic() || character == ' '))
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_table_has_exactly_35_actions_in_page_order() {
    let names: Vec<String> = action_rows().into_iter().map(|row| row.name).collect();
    assert_eq!(names, EXPECTED_NAMES.map(String::from).to_vec());
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_every_command_id_is_kernel_dot_the_action_name() {
    for row in action_rows() {
        assert_eq!(row.command, format!("kernel.{}", row.name));
    }
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_action_names_are_unique() {
    let names: BTreeSet<String> = action_rows().into_iter().map(|row| row.name).collect();
    assert_eq!(names.len(), 35);
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_only_rename_has_no_default_key() {
    let unbound: Vec<String> = action_rows()
        .into_iter()
        .filter(|row| row.default == "unbound")
        .map(|row| row.name)
        .collect();
    assert_eq!(unbound, vec!["rename".to_string()]);
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_default_keys_are_unique() {
    let keys: Vec<String> = action_rows()
        .into_iter()
        .map(|row| row.default)
        .filter(|key| key != "unbound")
        .collect();
    let set: BTreeSet<&String> = keys.iter().collect();
    assert_eq!(set.len(), keys.len());
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_screen_preset_keys_are_unique() {
    let keys: Vec<String> = action_rows().into_iter().map(|row| row.screen).collect();
    let set: BTreeSet<&String> = keys.iter().collect();
    assert_eq!(set.len(), keys.len());
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_tmux_preset_keys_are_unique() {
    let keys: Vec<String> = action_rows().into_iter().map(|row| row.tmux).collect();
    let set: BTreeSet<&String> = keys.iter().collect();
    assert_eq!(set.len(), keys.len());
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_every_key_is_in_the_bindable_set() {
    for row in action_rows() {
        for key in [&row.default, &row.screen, &row.tmux] {
            if key != "unbound" {
                assert!(bindable(key), "{} has the unbindable key {key}", row.name);
            }
        }
    }
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_tmux_preset_equals_the_default_table_except_for_rename() {
    for row in action_rows() {
        if row.name == "rename" {
            assert_eq!((row.default.as_str(), row.tmux.as_str()), ("unbound", ","));
        } else {
            assert_eq!(row.default, row.tmux, "{}", row.name);
        }
    }
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_screen_preset_differs_from_tmux_on_the_documented_actions() {
    let differing: Vec<String> = action_rows()
        .into_iter()
        .filter(|row| row.screen != row.tmux)
        .map(|row| row.name)
        .collect();
    assert_eq!(
        differing,
        vec![
            "close_pane",
            "new_split_view",
            "rename",
            "focus_tree",
            "focus_next_pane",
            "focus_previous_pane",
            "focus_pane_left",
            "focus_pane_right",
            "focus_pane_up",
            "focus_pane_down",
            "cycle_next_in_group",
            "cycle_previous_in_group",
            "jump_next_group",
            "jump_previous_group",
            "save",
            "run_command",
            "search",
            "quit",
            "settings",
        ]
    );
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_screen_preset_keys_match_the_user_guide() {
    let rows = action_rows();
    let key = |name: &str| {
        rows.iter()
            .find(|row| row.name == name)
            .expect("row")
            .screen
            .clone()
    };
    assert_eq!(key("new_terminal"), "c");
    assert_eq!(key("close_pane"), "k");
    assert_eq!(key("rename"), "A");
    assert_eq!(key("new_split_view"), "S");
    assert_eq!(key("focus_next_pane"), "n");
    assert_eq!(key("focus_previous_pane"), "p");
    assert_eq!(key("focus_pane_left"), "H");
    assert_eq!(key("focus_pane_down"), "J");
    assert_eq!(key("focus_pane_up"), "K");
    assert_eq!(key("focus_pane_right"), "L");
    assert_eq!(key("detach"), "d");
    assert_eq!(key("quit"), "\\");
    assert_eq!(key("settings"), "o");
    assert_eq!(key("run_command"), ":");
    assert_eq!(key("search"), "/");
    assert_eq!(key("focus_tree"), "t");
    assert_eq!(key("save"), "s");
    assert_eq!(key("cycle_next_in_group"), "Down");
    assert_eq!(key("cycle_previous_in_group"), "Up");
    assert_eq!(key("jump_next_group"), "PgDn");
    assert_eq!(key("jump_previous_group"), "PgUp");
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_tmux_preset_keys_match_the_user_guide() {
    let rows = action_rows();
    let key = |name: &str| {
        rows.iter()
            .find(|row| row.name == name)
            .expect("row")
            .tmux
            .clone()
    };
    assert_eq!(key("new_terminal"), "c");
    assert_eq!(key("close_pane"), "x");
    assert_eq!(key("rename"), ",");
    assert_eq!(key("new_split_view"), "\"");
    assert_eq!(key("focus_next_pane"), "o");
    assert_eq!(key("focus_previous_pane"), ";");
    assert_eq!(key("cycle_next_in_group"), "n");
    assert_eq!(key("cycle_previous_in_group"), "p");
    assert_eq!(key("jump_previous_group"), "(");
    assert_eq!(key("jump_next_group"), ")");
    assert_eq!(key("scrollback_up"), "[");
    assert_eq!(key("scrollback_down"), "]");
    assert_eq!(key("detach"), "d");
    assert_eq!(key("quit"), "&");
    assert_eq!(key("settings"), ":");
    assert_eq!(key("run_command"), "!");
    assert_eq!(key("search"), "f");
    assert_eq!(key("focus_tree"), "w");
    assert_eq!(key("save"), "S");
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_ilium_only_actions_keep_the_same_key_in_every_column() {
    let rows = action_rows();
    for name in [
        "new_agent_worktree",
        "new_editor",
        "new_board",
        "new_group",
        "new_folder",
        "toggle_move",
        "focus_pane",
        "scrollback_up",
        "scrollback_down",
        "Help",
        "toggle_editor_view_mode",
        "toggle_line_numbers",
        "toggle_minimap",
        "toggle_autosave",
    ] {
        let row = rows.iter().find(|row| row.name == name).expect("row");
        assert_eq!(row.default, row.screen, "{name}");
        assert_eq!(row.default, row.tmux, "{name}");
    }
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_help_action_is_bound_to_the_question_mark_in_all_columns() {
    let rows = action_rows();
    let row = rows
        .iter()
        .find(|row| row.name == "Help")
        .expect("Help row");
    assert_eq!(
        (row.default.as_str(), row.screen.as_str(), row.tmux.as_str()),
        ("?", "?", "?")
    );
    assert_eq!(row.command, "kernel.Help");
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_manifest_default_chord_equals_the_help_key() {
    let rows = action_rows();
    let help = rows
        .iter()
        .find(|row| row.name == "Help")
        .expect("Help row");
    assert_eq!(
        manifest()["contributes"]["keybindings"][0]["chord"],
        help.default.as_str()
    );
    assert_eq!(
        manifest()["contributes"]["settings"][0]["rows"][0]["default"],
        help.default.as_str()
    );
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_the_prefix_rules_are_stated_on_the_page() {
    let page = page_text();
    assert!(page
        .contains("`shortcut_base`, a `Ctrl` letter from `Ctrl+A` to `Ctrl+Z`, default `Ctrl+B`"));
    assert!(page.contains("`navigation_shortcut_base`"));
    assert!(
        page.contains("moving the general prefix to `Ctrl+A` keeps `Ctrl+B n` for tree traversal")
    );
    assert!(page.contains("Pressing the prefix twice passes it through to the program in the pane"));
    assert!(page.contains("The GNU Screen preset sets the prefix to `Ctrl+A`, and the tmux preset sets it to `Ctrl+B`"));
}

/// REQ: COR-011, F-007
#[test]
fn actions_011_four_tree_actions_use_the_navigation_prefix() {
    let page = page_text();
    for name in [
        "cycle_next_in_group",
        "cycle_previous_in_group",
        "jump_next_group",
        "jump_previous_group",
    ] {
        assert!(page.contains(&format!("`{name}`")));
    }
    assert!(page.contains("The four tree actions"));
}

proptest! {
    // REQ: COR-011, F-007
    #[test]
    fn actions_011_property_any_row_key_is_bindable_and_command_matches_name(index in 0usize..35) {
        let rows = action_rows();
        let row = &rows[index];
        prop_assert_eq!(&row.command, &format!("kernel.{}", row.name));
        prop_assert!(row.tmux.chars().count() <= 5);
        prop_assert!(bindable(&row.tmux));
    }

    // REQ: COR-011, F-007
    #[test]
    fn actions_011_property_preset_columns_never_assign_one_key_twice(a in 0usize..35, b in 0usize..35) {
        prop_assume!(a != b);
        let rows = action_rows();
        prop_assert_ne!(&rows[a].screen, &rows[b].screen);
        prop_assert_ne!(&rows[a].tmux, &rows[b].tmux);
    }
}
