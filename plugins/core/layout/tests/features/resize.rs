//! "Drag the divider": client converts the drag to a message, server submits `resize-pane`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Event, NodeId, PluginSide, PointerButton, PointerKind,
    PointerModifiers, ViewPointer, ViewTarget, ViewVisibilityEvent,
};
use proptest::prelude::*;

use crate::common::*;

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

fn drag_at(column: u16) -> ActionValue {
    ActionValue::Pointer(ViewPointer {
        kind: PointerKind::Drag,
        button: PointerButton::Left,
        modifiers: PointerModifiers(0),
        column,
        row: 0,
        sub_x: 128,
        sub_y: 128,
        clicks: 0,
    })
}

fn drag(host: &mut FakeHost, column: u16) {
    host.send_action(panel(), "preview", ActionKind::Pointer, drag_at(column));
    host.settle();
}

fn client_with_focus(focused: u64) -> FakeHost {
    let mut host = activated_client();
    host.deliver_one(Event::PaneFocused(Some(NodeId(focused))));
    host.deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
        view: panel(),
        visible: true,
    }));
    host
}

fn resize_messages(host: &FakeHost) -> Vec<ilium_plugin_sdk::types::MessageSpec> {
    messages(&host.submitted())
        .into_iter()
        .filter(|spec| spec.channel == "resize")
        .collect()
}

/// REQ: F-003
#[test]
fn f_003_divider_drag_messages_the_server_side_on_channel_resize() {
    let mut host = client_with_focus(3);
    drag(&mut host, 20);
    let sent = resize_messages(&host);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to.plugin.0, PLUGIN_ID);
    assert_eq!(sent[0].to.side, PluginSide::Server);
}

/// REQ: F-003
#[test]
fn f_003_resize_payload_is_focused_id_then_columns_as_10_little_endian_bytes() {
    let mut host = client_with_focus(3);
    drag(&mut host, 20);
    let payload = resize_messages(&host).remove(0).payload;
    assert_eq!(payload.len(), 10);
    assert_eq!(&payload[..8], &3u64.to_le_bytes());
    let columns = u16::from_le_bytes([payload[8], payload[9]]);
    assert!(
        columns >= 20,
        "columns = divider column x columns per cell, at least the divider column"
    );
}

/// REQ: F-003
#[test]
fn f_003_drag_while_the_view_is_hidden_sends_nothing() {
    let mut host = client_with_focus(3);
    host.deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
        view: panel(),
        visible: false,
    }));
    drag(&mut host, 20);
    assert!(resize_messages(&host).is_empty());
}

/// REQ: F-003
#[test]
fn f_003_drag_after_the_view_is_visible_again_sends_the_message() {
    let mut host = client_with_focus(3);
    host.deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
        view: panel(),
        visible: false,
    }));
    host.deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
        view: panel(),
        visible: true,
    }));
    drag(&mut host, 20);
    assert_eq!(resize_messages(&host).len(), 1);
}

/// REQ: F-003
#[test]
fn f_003_two_drags_send_two_messages() {
    let mut host = client_with_focus(3);
    drag(&mut host, 20);
    drag(&mut host, 22);
    assert_eq!(resize_messages(&host).len(), 2);
}

/// REQ: F-003
#[test]
fn f_003_server_turns_resize_message_into_resize_pane_with_columns_only() {
    let mut host = activated_server();
    let submissions = host.deliver_one(message_from_client("resize", resize_payload(3, 60)));
    let resized = resizes(&submissions);
    assert_eq!(resized.len(), 1);
    assert_eq!(resized[0].pane, NodeId(3));
    assert_eq!(resized[0].columns, Some(60));
    assert_eq!(resized[0].rows, None);
}

/// REQ: F-003
#[test]
fn f_003_server_resize_accepts_the_smallest_width() {
    let mut host = activated_server();
    let resized = resizes(&host.deliver_one(message_from_client("resize", resize_payload(3, 0))));
    assert_eq!(resized[0].columns, Some(0));
}

/// REQ: F-003
#[test]
fn f_003_server_resize_accepts_the_largest_width() {
    let mut host = activated_server();
    let resized =
        resizes(&host.deliver_one(message_from_client("resize", resize_payload(3, u16::MAX))));
    assert_eq!(resized[0].columns, Some(u16::MAX));
}

/// REQ: F-003
#[test]
fn f_003_refused_resize_leaves_the_plugin_running() {
    // A resize that the kernel refuses completes as refused; the plugin must not trap or disable itself.
    let mut host = activated_server();
    host.revoke("panes.lifecycle");
    host.deliver_one(message_from_client("resize", resize_payload(3, 60)));
    host.settle();
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // REQ: F-003
    #[test]
    fn f_003_server_resize_round_trips_pane_and_columns(pane in any::<u64>(), columns in any::<u16>()) {
        let mut host = activated_server();
        let resized = resizes(&host.deliver_one(message_from_client("resize", resize_payload(pane, columns))));
        prop_assert_eq!(resized.len(), 1);
        prop_assert_eq!(resized[0].pane, NodeId(pane));
        prop_assert_eq!(resized[0].columns, Some(columns));
        prop_assert_eq!(resized[0].rows, None);
    }
}
