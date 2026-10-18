//! Twins of the tests of ilium-client/src/keymap.rs. The keymap is kernel-native; the plugin owns
//! the Keyboard tab and the key reference, so each twin asserts what those two surfaces show or
//! write for the same input.

use serde_json::json;

use crate::common::*;

fn accepted_prefix(text: &str) -> bool {
    let mut host = column_host(Column::Tmux);
    host.send_action(
        keyboard_page(),
        "shortcut_base",
        ilium_plugin_sdk::prelude::ActionKind::Changed,
        ilium_plugin_sdk::prelude::ActionValue::Text(text.to_string()),
    );
    host.settle();
    host.kernel_settings().writes().len() == 1
}

fn prefix_footer(base: &str) -> String {
    let mut host = host_with_tab(
        json!({"shortcut_base": base, "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
    );
    set_keymap(
        &mut host,
        vec![binding("Panes", "Split vertically", "prefix+%", true)],
        &[("prefix+%", "Prefix  %")],
    );
    render_text(&mut host, keys_panel(), 120, 14)
}

const ADVICE: [(char, &str); 26] = [
    ('a', "GNU Screen's established prefix; it shadows shell beginning-of-line while ilium is active."),
    ('b', "tmux's established prefix; it shadows shell backward-character while ilium is active."),
    ('c', "commonly sends interrupt/SIGINT and is often used for terminal copy"),
    ('d', "commonly sends EOF or logs out of an interactive shell"),
    ('e', "commonly moves the cursor to the end of the line"),
    ('f', "commonly moves forward one character or opens search"),
    ('g', "commonly cancels the current operation or rings the terminal bell"),
    ('h', "is normally Backspace in terminal control encoding"),
    ('i', "is indistinguishable from Tab in ordinary terminals, so every Tab becomes the shortcut base"),
    ('j', "is normally line-feed/newline in terminal control encoding"),
    ('k', "commonly cuts from the cursor to the end of the line"),
    ('l', "commonly clears or redraws the terminal"),
    ('m', "is indistinguishable from Enter in ordinary terminals, so every Enter becomes the shortcut base"),
    ('n', "commonly selects the next history entry or search result"),
    ('o', "commonly accepts the current line and fetches the next history entry"),
    ('p', "commonly selects the previous history entry"),
    ('q', "commonly resumes software flow control"),
    ('r', "commonly starts reverse history search"),
    ('s', "commonly pauses software flow control and is also used for Save"),
    ('t', "commonly transposes characters and is often used for a new terminal tab"),
    ('u', "commonly erases from the cursor to the start of the line"),
    ('v', "commonly quotes the next key and is often used for terminal paste"),
    ('w', "commonly erases the previous word and is often used to close a tab"),
    ('x', "is an Emacs command prefix and is often used for Cut"),
    ('y', "commonly yanks previously cut text"),
    ('z', "commonly suspends the foreground process and is often used for Undo"),
];

/// REQ: PORT:ilium-client/src/keymap.rs::shortcut_base_accepts_exactly_one_ascii_letter
#[test]
fn keymap_port_shortcut_base_accepts_exactly_one_ascii_letter() {
    assert!(accepted_prefix("ctrl+a"));
    assert!(accepted_prefix("ctrl+b"));
    for refused in ["ctrl+", "ctrl+ab", "ctrl+?", "ctrl+é", ""] {
        assert!(!accepted_prefix(refused), "{refused:?} must be refused");
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::shortcut_base_steps_through_all_letters_with_wrapping
#[test]
fn keymap_port_shortcut_base_covers_all_26_letters_and_both_ends_display() {
    for letter in 'a'..='z' {
        assert!(accepted_prefix(&format!("ctrl+{letter}")), "{letter}");
    }
    assert!(prefix_footer("ctrl+z").contains("Prefix is Ctrl+Z."));
    assert!(prefix_footer("ctrl+a").contains("Prefix is Ctrl+A."));
}

/// REQ: PORT:ilium-client/src/keymap.rs::every_allowed_letter_has_specific_advice
#[test]
fn keymap_port_every_allowed_letter_has_specific_advice_on_the_keyboard_tab() {
    for (letter, advice) in ADVICE {
        let mut host = host_with_tab(
            json!({"shortcut_base": format!("ctrl+{letter}"), "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
        );
        let text = render_text(&mut host, keyboard_page(), 160, 60);
        assert!(
            text.contains(advice),
            "advice of Ctrl+{letter} missing: {text}"
        );
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::is_leader_key_matches_configured_ctrl_a
#[test]
fn keymap_port_the_leader_is_ctrl_a_when_configured() {
    assert!(prefix_footer("ctrl+a").contains("Prefix is Ctrl+A."));
}

/// REQ: PORT:ilium-client/src/keymap.rs::is_leader_key_rejects_plain_a
#[test]
fn keymap_port_the_leader_is_never_the_plain_letter() {
    let text = prefix_footer("ctrl+a");
    assert!(!text.contains("Prefix is A."), "{text}");
    assert!(!text.contains("Prefix is a."), "{text}");
}

/// REQ: PORT:ilium-client/src/keymap.rs::is_leader_key_changes_with_the_configured_base
#[test]
fn keymap_port_the_leader_changes_with_the_configured_base() {
    let ctrl_b = prefix_footer("ctrl+b");
    assert!(ctrl_b.contains("Prefix is Ctrl+B."));
    assert!(!ctrl_b.contains("Prefix is Ctrl+A."));
}

/// REQ: PORT:ilium-client/src/keymap.rs::ctrl_i_and_ctrl_m_terminal_ambiguities_remain_selectable
#[test]
fn keymap_port_ctrl_i_and_ctrl_m_remain_selectable() {
    assert!(accepted_prefix("ctrl+i"));
    assert!(accepted_prefix("ctrl+m"));
    assert!(prefix_footer("ctrl+i").contains("Prefix is Ctrl+I."));
    assert!(prefix_footer("ctrl+m").contains("Prefix is Ctrl+M."));
}

/// REQ: PORT:ilium-client/src/keymap.rs::action_for_known_letter
#[test]
fn keymap_port_known_letters_are_rows_of_their_actions() {
    let mut host = column_host(Column::Tmux);
    let screen = host.render(keyboard_page(), 80, 60);
    for (name, key) in [("detach", "d"), ("new_terminal", "c"), ("run_command", "!")] {
        let (row, _) = screen.find(name).unwrap_or_else(|| panic!("{name} row"));
        let line = screen.row(row);
        assert!(
            line.trim_end_matches([' ', '│']).contains(key),
            "{name}: {line}"
        );
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::action_for_unknown_letter
#[test]
fn keymap_port_an_unused_letter_is_free_to_capture() {
    let mut host = column_host(Column::Tmux);
    activate(&mut host, "new_terminal");
    write_chord(&mut host, "z");
    host.settle();
    assert!(!render_text(&mut host, keyboard_page(), 120, 60).contains("used by"));
}

/// REQ: PORT:ilium-client/src/keymap.rs::action_from_name_rejects_an_unknown_name
#[test]
fn keymap_port_an_unknown_action_name_opens_no_capture() {
    let mut host = column_host(Column::Tmux);
    activate(&mut host, "not_a_real_action");
    assert!(!render_text(&mut host, keyboard_page(), 120, 60).contains("Press the new key chord."));
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: PORT:ilium-client/src/keymap.rs::action_name_round_trips_through_action_from_name_for_every_binding
#[test]
fn keymap_port_every_action_name_of_the_table_opens_its_own_capture() {
    for row in action_rows() {
        let mut host = column_host(Column::Tmux);
        activate(&mut host, &row.name);
        assert!(
            render_text(&mut host, keyboard_page(), 120, 80).contains("Press the new key chord."),
            "{} does not open a capture",
            row.name
        );
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::assignment_refuses_a_key_already_owned_by_another_action
#[test]
fn keymap_port_a_key_owned_by_another_action_is_refused_and_the_owner_is_named() {
    let mut host = tmux_host();
    set_keymap(
        &mut host,
        vec![
            binding("Keyboard", "New terminal", "prefix+c", true),
            binding("Keyboard", "Quit", "prefix+&", true),
        ],
        &[("prefix+c", "c"), ("prefix+&", "&")],
    );
    activate(&mut host, "Quit");
    write_chord(&mut host, "c");
    host.key(keyboard_page(), "Enter");
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
    assert!(render_text(&mut host, keyboard_page(), 120, 20).contains("used by New terminal"));
    assert!(render_text(&mut host, keyboard_page(), 120, 20).contains('&'));
}

/// REQ: PORT:ilium-client/src/keymap.rs::default_keys_follow_tmux_for_navigation
#[test]
fn keymap_port_default_keys_follow_tmux_for_navigation() {
    let mut host = column_host(Column::Default);
    set_tab(&mut host, json!({}), "shortcut_base");
    assert!(render_text(&mut host, keyboard_page(), 80, 80).contains("Ctrl+B"));
    let screen = host.render(keyboard_page(), 80, 80);
    for (name, key) in [
        ("cycle_next_in_group", "n"),
        ("cycle_previous_in_group", "p"),
        ("jump_next_group", ")"),
        ("jump_previous_group", "("),
        ("focus_pane_left", "Left"),
        ("focus_pane_right", "Right"),
        ("focus_pane_up", "Up"),
        ("focus_pane_down", "Down"),
        ("focus_next_pane", "o"),
        ("focus_previous_pane", ";"),
    ] {
        let (row, _) = screen.find(name).unwrap_or_else(|| panic!("{name} row"));
        let line = screen.row(row);
        assert!(
            line.trim_end_matches([' ', '│']).ends_with(key),
            "{name}: {line}"
        );
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::tmux_preset_equals_the_default_table
#[test]
fn keymap_port_the_tmux_column_equals_the_default_column_except_rename() {
    for row in action_rows() {
        if row.name == "rename" {
            assert_eq!(row.default, "unbound");
            assert_eq!(row.tmux, ",");
        } else {
            assert_eq!(row.default, row.tmux, "{}", row.name);
        }
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::presets_have_their_documented_prefixes_and_no_collisions
#[test]
fn keymap_port_presets_have_their_documented_prefixes_and_no_collisions() {
    for column in [Column::Default, Column::Screen, Column::Tmux] {
        let keys: Vec<String> = action_rows()
            .iter()
            .map(|row| column.key(row).to_string())
            .filter(|key| key != "unbound")
            .collect();
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }
    let mut screen = column_host(Column::Screen);
    assert!(render_text(&mut screen, keyboard_page(), 80, 60).contains("Ctrl+A"));
    let mut tmux = column_host(Column::Tmux);
    assert!(render_text(&mut tmux, keyboard_page(), 80, 60).contains("Ctrl+B"));
    let close = action_rows()
        .into_iter()
        .find(|row| row.name == "close_pane")
        .expect("close_pane");
    assert_eq!(close.tmux, "x");
}

fn mnemonic_words_of_row(text: &str, name: &str, key: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| line.contains(name))
        .unwrap_or("")
        .replace(name, "");
    let _ = key;
    line.split(|c: char| !c.is_alphabetic())
        .filter(|word| word.len() >= 2)
        .map(str::to_string)
        .collect()
}

/// REQ: PORT:ilium-client/src/keymap.rs::mnemonic_words_start_with_their_registered_letters
#[test]
fn keymap_port_mnemonic_words_start_with_the_key_they_explain() {
    let mut host = column_host(Column::Tmux);
    let text = render_text(&mut host, keyboard_page(), 160, 80);
    for row in action_rows() {
        let Some(letter) = row
            .tmux
            .chars()
            .next()
            .filter(|c| row.tmux.len() == 1 && c.is_ascii_alphabetic())
        else {
            continue;
        };
        let words = mnemonic_words_of_row(&text, &row.name, &row.tmux);
        assert!(
            words.iter().any(|word| word
                .to_ascii_lowercase()
                .starts_with(letter.to_ascii_lowercase())),
            "{}: no mnemonic word for {letter} in {words:?}",
            row.name
        );
    }
}

/// REQ: PORT:ilium-client/src/keymap.rs::documented_preset_letters_have_mnemonics_except_explicitly_unmnemonicable_keys
#[test]
fn keymap_port_documented_preset_letters_have_mnemonics_except_the_unmnemonicable_keys() {
    for column in [Column::Default, Column::Screen, Column::Tmux] {
        let mut host = column_host(column);
        let text = render_text(&mut host, keyboard_page(), 160, 80);
        for row in action_rows() {
            let key = column.key(&row).to_string();
            let Some(letter) = key
                .chars()
                .next()
                .filter(|c| key.len() == 1 && c.is_ascii_alphabetic())
            else {
                continue;
            };
            if row.name == "focus_pane_up" && letter.eq_ignore_ascii_case(&'k') {
                continue;
            }
            let words = mnemonic_words_of_row(&text, &row.name, &key);
            assert!(
                words.iter().any(|word| word
                    .to_ascii_lowercase()
                    .starts_with(letter.to_ascii_lowercase())),
                "{} on {letter} needs a mnemonic: {words:?}",
                row.name
            );
        }
    }
    // The Screen preset binds search to `/`, which has no mnemonic word.
    let mut screen = column_host(Column::Screen);
    let text = render_text(&mut screen, keyboard_page(), 160, 80);
    assert!(mnemonic_words_of_row(&text, "search", "/").is_empty());
}
