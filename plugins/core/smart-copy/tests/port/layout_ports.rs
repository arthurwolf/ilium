//! Plugin-level twins of context_menu_layout.rs and the ui.rs preview test: the menu viewport
//! arithmetic stays with the host, so the twins assert what the plugin contributes to it (the
//! ordered entries that must all stay reachable) and that the picker never leaks a scene name.

use ilium_plugin_sdk::exports::MenuContext;
use ilium_plugin_sdk::types::NodeId;

use crate::common::{add_terminal_pane, client_host, open_picker, picker_target};

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

/// REQ: PORT:ilium-client/src/context_menu_layout.rs::padding_and_group_boundaries_never_activate_an_action, PORT:ilium-client/src/context_menu_layout.rs::every_action_remains_visible_and_clickable_on_a_short_menu
#[test]
fn port_layout_every_terminal_menu_action_is_offered_once_in_order() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let entries = host.menu_items(terminal_context(pane));
    let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, ["copy", "copy-path", "smart-copy", "transfer"]);
    assert!(entries.iter().all(|entry| !entry.label.is_empty()));
}

/// REQ: PORT:ilium-client/src/context_menu_layout.rs::hovering_a_visible_action_does_not_move_the_viewport
#[test]
fn port_layout_asking_twice_returns_the_same_entries() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    let first: Vec<String> = host
        .menu_items(terminal_context(pane))
        .into_iter()
        .map(|e| e.id)
        .collect();
    let second: Vec<String> = host
        .menu_items(terminal_context(pane))
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(first, second);
}

/// REQ: PORT:ilium-client/src/ui.rs::smart_copy_preview_withholds_scene_attribution_and_recovers_after_close
#[test]
fn port_ui_picker_view_shows_candidates_without_scene_attribution() {
    let mut host = open_picker(&["alpha", "beta"]);
    let text = host.render(picker_target(), 40, 9).text();
    assert!(text.contains("alpha"));
    assert!(text.contains("beta"));
    assert!(!text.to_lowercase().contains("scene"));
}
