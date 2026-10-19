//! F-016: the terminal right-click menu. The `menu-provider` export answers `menu-items` for the
//! context menu declared in the manifest (page sample "Terminal right-click menu").

use ilium_plugin_sdk::exports::{MenuContext, MenuEntry};
use ilium_plugin_sdk::primitives::terminal::Selection;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::NodeId;

use crate::common::{add_terminal_pane, client_host};

fn context(pane: Option<u64>) -> MenuContext {
    MenuContext {
        menu: "terminal".to_string(),
        node: pane.map(NodeId),
        pane: pane.map(NodeId),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    }
}

fn selection(text: &str) -> Selection {
    Selection {
        first_row: 1,
        first_column: 2,
        last_row: 1,
        last_column: 9,
        text: text.to_string(),
    }
}

fn menu(host: &mut FakeHost, pane: Option<u64>) -> Vec<MenuEntry> {
    host.menu_items(context(pane))
}

fn entry<'a>(entries: &'a [MenuEntry], id: &str) -> &'a MenuEntry {
    entries
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("entry {id} missing"))
}

/// REQ: F-016
#[test]
fn feature_016_menu_lists_copy_copy_path_smart_copy_and_transfer_in_that_order() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let ids: Vec<String> = menu(&mut host, Some(pane))
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(ids, ["copy", "copy-path", "smart-copy", "transfer"]);
}

/// REQ: F-016
#[test]
fn feature_016_menu_labels_are_the_ones_of_the_page_figure() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let labels: Vec<String> = menu(&mut host, Some(pane))
        .into_iter()
        .map(|entry| entry.label)
        .collect();
    assert_eq!(
        labels,
        ["Copy", "Copy path", "Smart Copy", "Transfer screen"]
    );
}

/// REQ: F-016
#[test]
fn feature_016_copy_is_disabled_with_a_reason_when_nothing_is_selected() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    assert_eq!(
        entry(&entries, "copy").disabled_reason.as_deref(),
        Some("nothing selected")
    );
}

/// REQ: F-016
#[test]
fn feature_016_copy_is_enabled_when_the_selection_has_text() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal()
        .set_selection(pane, selection("modified: src/lib.rs"));
    let entries = menu(&mut host, Some(pane));
    assert_eq!(entry(&entries, "copy").disabled_reason, None);
}

/// REQ: F-016
#[test]
fn feature_016_copy_stays_disabled_for_an_empty_selection_text() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_selection(pane, selection(""));
    let entries = menu(&mut host, Some(pane));
    assert_eq!(
        entry(&entries, "copy").disabled_reason.as_deref(),
        Some("nothing selected")
    );
}

/// REQ: F-016
#[test]
fn feature_016_copy_reason_fits_the_forty_character_limit() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    let reason = entry(&entries, "copy")
        .disabled_reason
        .clone()
        .expect("disabled");
    assert!(reason.chars().count() <= 40);
}

/// REQ: F-016
#[test]
fn feature_016_a_selection_on_another_pane_does_not_enable_copy() {
    let mut host = client_host();
    let first = add_terminal_pane(&mut host, "api");
    let second = add_terminal_pane(&mut host, "web");
    host.terminal().set_selection(first, selection("abc"));
    let entries = menu(&mut host, Some(second));
    assert_eq!(
        entry(&entries, "copy").disabled_reason.as_deref(),
        Some("nothing selected")
    );
}

/// REQ: F-016
#[test]
fn feature_016_smart_copy_entry_carries_the_ctrl_shift_c_shortcut() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    assert_eq!(
        entry(&entries, "smart-copy").shortcut.as_deref(),
        Some("ctrl+shift+c")
    );
}

/// REQ: F-016
#[test]
fn feature_016_transfer_screen_is_separated_from_the_copy_entries() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    assert!(entry(&entries, "transfer").separator_before);
    for id in ["copy", "copy-path", "smart-copy"] {
        assert!(!entry(&entries, id).separator_before, "{id}");
    }
}

/// REQ: F-016
#[test]
fn feature_016_copy_path_and_smart_copy_are_always_enabled() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    assert_eq!(entry(&entries, "copy-path").disabled_reason, None);
    assert_eq!(entry(&entries, "smart-copy").disabled_reason, None);
}

/// REQ: F-016
#[test]
fn feature_016_menu_without_a_pane_has_no_entries() {
    let mut host = client_host();
    assert!(menu(&mut host, None).is_empty());
}

/// REQ: F-016
#[test]
fn feature_016_menu_has_at_most_sixty_four_entries_and_no_submenus() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    assert!(entries.len() <= 64);
    assert!(entries
        .iter()
        .all(|entry| !entry.is_submenu && entry.parent.is_none()));
}

/// REQ: F-016
#[test]
fn feature_016_menu_entry_ids_are_unique() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = menu(&mut host, Some(pane));
    let mut ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), entries.len());
}

/// REQ: F-016
#[test]
fn feature_016_the_menu_answers_for_a_pane_that_is_not_in_the_tree_without_failing() {
    let mut host = client_host();
    let entries = menu(&mut host, Some(9999));
    assert!(entries.len() <= 64);
    assert!(host.failures().is_empty());
}
