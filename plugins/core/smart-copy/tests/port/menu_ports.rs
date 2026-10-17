//! Ports of app.rs menu tests, context_menu_layout.rs and ui.rs preview.

use ilium_plugin_sdk::exports::MenuContext;
use ilium_plugin_sdk::primitives::terminal::Selection;
use ilium_plugin_sdk::types::NodeId;

use crate::common::{add_terminal_pane, client_host};

fn context(pane: u64) -> MenuContext {
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

/// REQ: PORT:ilium-client/src/app.rs::right_click_menu_offers_copy_selection_only_when_the_pane_has_one
#[test]
fn port_menu_copy_is_enabled_only_when_the_pane_has_a_selection() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let before = host.menu_items(context(pane));
    assert_eq!(before[0].id, "copy");
    assert_eq!(
        before[0].disabled_reason.as_deref(),
        Some("nothing selected")
    );
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
    let after = host.menu_items(context(pane));
    assert_eq!(after[0].id, "copy");
    assert_eq!(after[0].disabled_reason, None);
}

/// REQ: PORT:ilium-client/src/app.rs::right_click_opens_the_terminal_menu_even_when_the_agent_tracks_the_mouse
#[test]
fn port_menu_is_offered_for_a_terminal_pane_regardless_of_mouse_tracking() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "agent");
    let ids: Vec<String> = host
        .menu_items(context(pane))
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, ["copy", "copy-path", "smart-copy", "transfer"]);
}
