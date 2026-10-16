//! Twins of the source-viewport wheel tests of ilium-client/src/editor_pane.rs and
//! ilium-client/tests/editor_navigation.rs, and of the dirty-flag tests of ilium-client/src/app.rs
//! and ui.rs, through the rendered pane, pointer events and the buffers service (F-019, F-020).

use crate::helpers::plain_host;
use crate::support::{call_buffers, direct, direct_open, done_json, render_pane, seed_text};
use ilium_plugin_sdk::generated::types::{
    BufferId, EditorPointerEvent, Event, NodeId, PointerButton, PointerKind, PointerModifiers,
    TextPosition, ViewPointer, ViewTarget,
};
use ilium_plugin_sdk::testing::host::FakeHost;
use serde_json::json;

fn ten_lines() -> String {
    (0..10).map(|row| format!("line {row}\n")).collect()
}

fn wheel(kind: PointerKind) -> Event {
    Event::EditorPointer(EditorPointerEvent {
        buffer: BufferId(1),
        pane: NodeId(7),
        position: TextPosition { line: 0, column: 0 },
        pointer: ViewPointer {
            kind,
            button: PointerButton::Left,
            modifiers: PointerModifiers(0),
            column: 2,
            row: 2,
            sub_x: 0,
            sub_y: 0,
            clicks: 0,
        },
    })
}

fn scrolling_host() -> FakeHost {
    plain_host("scroll.txt", None, &ten_lines(), &[("show_line_numbers", json!(false))])
}

/// REQ: PORT:ilium-client/tests/editor_navigation.rs::source_pane_wheel_event_routes_to_the_editor_viewport
#[test]
fn port_navigation_one_wheel_step_moves_the_source_viewport_by_three_rows() {
    let mut host = scrolling_host();
    host.deliver_one(wheel(PointerKind::WheelDown));
    let screen = render_pane(&mut host, 30, 6);
    assert!(screen.find("line 3").is_some(), "{}", screen.text());
    assert!(screen.find("line 2").is_none(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::source_wheel_scroll_moves_and_clamps_the_independent_viewport, PORT:ilium-client/tests/editor_navigation.rs::wheel_navigation_moves_the_source_viewport_and_keeps_it_until_cursor_navigation
#[test]
fn port_navigation_wheel_clamps_at_the_last_page_and_at_the_top() {
    let mut host = scrolling_host();
    for _ in 0..40 {
        host.deliver_one(wheel(PointerKind::WheelDown));
    }
    let bottom = render_pane(&mut host, 30, 6);
    assert!(bottom.find("line 9").is_some(), "{}", bottom.text());
    assert!(bottom.find("line 0").is_none(), "{}", bottom.text());
    for _ in 0..40 {
        host.deliver_one(wheel(PointerKind::WheelUp));
    }
    let top = render_pane(&mut host, 30, 6);
    assert!(top.find("line 0").is_some(), "{}", top.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::source_wheel_scroll_moves_and_clamps_the_independent_viewport
#[test]
fn port_navigation_wheel_position_survives_a_second_render() {
    let mut host = scrolling_host();
    host.deliver_one(wheel(PointerKind::WheelDown));
    let first = render_pane(&mut host, 30, 6);
    let second = render_pane(&mut host, 30, 6);
    assert_eq!(first.text(), second.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::wheel_scroll_does_not_move_the_cursor_in_highlighted_files
#[test]
fn port_navigation_wheel_in_a_highlighted_file_scrolls_and_leaves_the_text_unchanged() {
    let mut host = plain_host("scroll.rs", Some("rust"), &(0..10).map(|r| format!("let line_{r} = {r};\n")).collect::<String>(), &[("show_line_numbers", json!(false))]);
    host.deliver_one(wheel(PointerKind::WheelDown));
    let screen = render_pane(&mut host, 40, 6);
    assert!(screen.find("line_3").is_some(), "{}", screen.text());
    assert!(screen.find("line_0").is_none(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::cursor_navigation_reveals_cursor_after_source_wheel_scroll, PORT:ilium-client/tests/editor_navigation.rs::wheel_navigation_moves_the_source_viewport_and_keeps_it_until_cursor_navigation
#[test]
fn port_navigation_cursor_navigation_reveals_the_cursor_after_a_wheel_scroll() {
    let mut host = scrolling_host();
    for _ in 0..40 {
        host.deliver_one(wheel(PointerKind::WheelDown));
    }
    host.key(ViewTarget::Pane(NodeId(7)), "ctrl+home");
    let screen = render_pane(&mut host, 30, 6);
    assert!(screen.find("line 0").is_some(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::save_without_path_errors
#[test]
fn port_pane_save_of_a_pathless_buffer_submits_nothing() {
    let mut host = crate::support::active_host(&[]);
    host.deliver_one(Event::EditorOpened(crate::support::buffer_ref(1, None, None, 1)));
    host.send_action(
        ViewTarget::Pane(NodeId(7)),
        "save",
        ilium_plugin_sdk::generated::types::ActionKind::Activated,
        ilium_plugin_sdk::generated::types::ActionValue::None,
    );
    assert!(host.submitted().is_empty(), "{:?}", host.submitted());
}

fn dirty_of(editor: &mut ilium_plugin_core_editor::EditorClient) -> serde_json::Value {
    let listed = done_json(call_buffers(editor, "list", json!({})));
    listed
        .get("buffers")
        .and_then(|buffers| buffers.get(0))
        .and_then(|buffer| buffer.get("dirty"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// REQ: PORT:ilium-client/src/app.rs::close_confirmation_is_none_for_a_clean_editor, PORT:ilium-client/src/ui.rs::editor_identity_why_reports_the_authoritative_dirty_flag
#[test]
fn port_dirty_flag_of_a_clean_buffer_is_false() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    assert_eq!(dirty_of(&mut editor), json!(false));
}

/// REQ: PORT:ilium-client/src/app.rs::close_confirmation_warns_about_a_dirty_editor, PORT:ilium-client/src/ui.rs::editor_identity_why_reports_the_authoritative_dirty_flag
#[test]
fn port_dirty_flag_of_an_edited_buffer_is_true() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "changed");
    assert_eq!(dirty_of(&mut editor), json!(true));
}
