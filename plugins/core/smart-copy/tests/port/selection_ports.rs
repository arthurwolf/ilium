//! Ports of terminal_selection.rs: the selection text the plugin reads back through the menu.

use ilium_plugin_sdk::exports::MenuContext;
use ilium_plugin_sdk::primitives::terminal::Selection;
use ilium_plugin_sdk::types::NodeId;

use crate::common::{add_terminal_pane, client_host};

fn copy_reason(text: &str, rows: (u32, u32, u32, u32)) -> Option<String> {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_selection(
        pane,
        Selection {
            first_row: rows.0,
            first_column: rows.1 as u16,
            last_row: rows.2,
            last_column: rows.3 as u16,
            text: text.to_string(),
        },
    );
    let context = MenuContext {
        menu: "terminal".to_string(),
        node: Some(NodeId(pane)),
        pane: Some(NodeId(pane)),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    };
    host.menu_items(context).remove(0).disabled_reason
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::a_fresh_selection_is_empty
#[test]
fn port_selection_an_empty_selection_leaves_copy_disabled() {
    assert_eq!(
        copy_reason("", (0, 0, 0, 0)).as_deref(),
        Some("nothing selected")
    );
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::extracts_text_within_a_single_row
#[test]
fn port_selection_a_single_row_selection_enables_copy() {
    assert_eq!(copy_reason("hello", (0, 0, 0, 4)), None);
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::extracts_text_spanning_multiple_rows
#[test]
fn port_selection_a_multi_row_selection_enables_copy() {
    assert_eq!(copy_reason("line\nsecond line\nthird", (0, 6, 2, 4)), None);
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::normalized_orders_a_backward_drag
#[test]
fn port_selection_a_backward_selection_enables_copy() {
    assert_eq!(copy_reason("hello", (0, 4, 0, 0)), None);
}

fn terminal_context(pane: u64) -> MenuContext {
    MenuContext {
        menu: "terminal".to_string(),
        node: Some(NodeId(pane)),
        pane: Some(NodeId(pane)),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    }
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::a_fresh_selection_is_empty
#[test]
fn port_selection_a_pane_never_selected_has_copy_disabled_without_any_selection_set() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = host.menu_items(terminal_context(pane));
    assert_eq!(
        entries[0].disabled_reason.as_deref(),
        Some("nothing selected")
    );
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::extracts_text_spanning_multiple_rows
#[test]
fn port_selection_a_multi_row_selection_keeps_copy_as_the_first_entry_label() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_selection(
        pane,
        Selection {
            first_row: 0,
            first_column: 6,
            last_row: 2,
            last_column: 4,
            text: "line\nsecond line\nthird".to_string(),
        },
    );
    let entries = host.menu_items(terminal_context(pane));
    assert_eq!(entries[0].label.to_string(), "Copy");
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::extracts_text_within_a_single_row
#[test]
fn port_selection_a_single_row_selection_leaves_copy_path_enabled() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_selection(
        pane,
        Selection {
            first_row: 0,
            first_column: 0,
            last_row: 0,
            last_column: 4,
            text: "hello".to_string(),
        },
    );
    let entries = host.menu_items(terminal_context(pane));
    assert_eq!(entries[1].id, "copy-path");
    assert_eq!(entries[1].disabled_reason, None);
}

/// REQ: PORT:ilium-client/src/terminal_selection.rs::normalized_orders_a_backward_drag
#[test]
fn port_selection_a_backward_multi_row_selection_enables_copy() {
    assert_eq!(copy_reason("a\nb\nc", (2, 0, 0, 4)), None);
}
