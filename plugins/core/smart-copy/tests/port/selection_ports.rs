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
