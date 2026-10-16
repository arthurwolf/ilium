//! "Follow focus and size": the client tracks pane sizes and the focused title.

use ilium_plugin_sdk::types::{Event, NodeId, PaneResizedEvent, ViewTarget};

use crate::common::*;

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// REQ: F-003
#[test]
fn f_003_pane_resized_event_updates_the_size_row_of_the_panel() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(Event::PaneResized(PaneResizedEvent { node: NodeId(pane), columns: 80, rows: 24 }));
    host.settle();
    let screen = host.render(panel(), 48, 7);
    assert!(screen.text().contains("80x24"));
}

/// REQ: F-003
#[test]
fn f_003_a_second_resize_of_the_same_pane_replaces_its_size() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(Event::PaneResized(PaneResizedEvent { node: NodeId(pane), columns: 80, rows: 24 }));
    host.deliver_one(Event::PaneResized(PaneResizedEvent { node: NodeId(pane), columns: 61, rows: 12 }));
    host.settle();
    let text = host.render(panel(), 48, 7).text();
    assert!(text.contains("61x12"));
    assert!(!text.contains("80x24"));
}

/// REQ: F-003
#[test]
fn f_003_pane_focused_reads_the_pane_title_into_the_panel() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(Event::PaneFocused(Some(NodeId(pane))));
    host.settle();
    assert!(host.render(panel(), 48, 7).text().contains("api"));
}

/// REQ: F-003
#[test]
fn f_003_pane_focused_shows_the_renamed_title() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.tree().rename(pane, "gateway");
    host.deliver_one(Event::PaneFocused(Some(NodeId(pane))));
    host.settle();
    assert!(host.render(panel(), 48, 7).text().contains("gateway"));
}

/// REQ: F-003
#[test]
fn f_003_focus_cleared_keeps_the_last_title() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(Event::PaneFocused(Some(NodeId(pane))));
    host.deliver_one(Event::PaneFocused(None));
    host.settle();
    assert!(host.render(panel(), 48, 7).text().contains("api"));
}

/// REQ: F-003
#[test]
fn f_003_events_for_the_tracker_produce_no_submissions() {
    let mut host = activated_client();
    let pane = host.tree().add(pane_node("api", "ilium.terminal", None));
    let submissions = host.deliver(vec![
        Event::PaneResized(PaneResizedEvent { node: NodeId(pane), columns: 80, rows: 24 }),
        Event::PaneFocused(Some(NodeId(pane))),
    ]);
    assert!(submissions.is_empty());
}
