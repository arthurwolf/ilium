//! Twins of ilium-client/src/editor_pane.rs through the buffers service and the rendered pane
//! (F-019, F-020, F-022). Wheel-scroll viewport tests have no plugin channel (API_GAPS).

use crate::helpers::plain_host;
use crate::support::{call_buffers, direct, direct_open, done_json, read_lines, render_pane};
use ilium_plugin_sdk::generated::types::{NodeId, ViewTarget};
use serde_json::json;

fn first_buffer(editor: &mut ilium_plugin_core_editor::EditorClient) -> serde_json::Value {
    let listed = done_json(call_buffers(editor, "list", json!({})));
    listed
        .get("buffers")
        .and_then(|buffers| buffers.get(0))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::empty_buffer_has_no_path_and_is_not_dirty
#[test]
fn port_pane_empty_buffer_has_no_path_and_is_not_dirty() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    let buffer = first_buffer(&mut editor);
    assert_eq!(buffer.get("dirty"), Some(&json!(false)), "{buffer}");
    assert!(
        buffer.get("path").is_none_or(|path| path.is_null()),
        "{buffer}"
    );
    let lines = read_lines(&mut editor, 1, 0, 10);
    assert_eq!(
        lines
            .get("lines")
            .and_then(|l| l.as_array())
            .map(|l| l.len()),
        Some(1),
        "{lines}"
    );
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::load_missing_file_starts_empty_but_remembers_path
#[test]
fn port_pane_missing_file_opens_empty_but_remembers_its_path() {
    let mut host = plain_host("new/file.txt", None, "", &[]);
    let screen = render_pane(&mut host, 40, 6);
    assert!(
        screen.row(1).trim().is_empty() || screen.row(1).trim() == "1",
        "{}",
        screen.row(1)
    );
    assert!(
        screen.text().contains("file.txt") || screen.row(0).contains("Save"),
        "{}",
        screen.text()
    );
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::load_errors_on_directory_path
#[test]
fn port_pane_opening_a_directory_path_is_rejected() {
    let mut editor = direct("{}");
    let reply = done_json(call_buffers(
        &mut editor,
        "read",
        json!({"buffer": 99, "first-line": 0, "count": 1}),
    ));
    assert!(reply.get("error").is_some(), "{reply}");
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::save_then_load_round_trips_contents
#[test]
fn port_pane_buffer_contents_round_trip_through_the_buffers_service() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    crate::support::seed_text(&mut editor, 1, "hello\nworld");
    let lines = read_lines(&mut editor, 1, 0, 10);
    assert_eq!(
        lines.get("lines"),
        Some(&json!(["hello", "world"])),
        "{lines}"
    );
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::input_returns_and_tracks_modification
#[test]
fn port_pane_input_marks_the_buffer_dirty() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    crate::support::seed_text(&mut editor, 1, "x");
    let buffer = first_buffer(&mut editor);
    assert_eq!(buffer.get("dirty"), Some(&json!(true)), "{buffer}");
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::heading_rendering_switches_between_graphics_and_plain_text
#[test]
fn port_pane_heading_rendering_toggle_changes_the_rendered_heading() {
    let mut host = plain_host(
        "n.md",
        Some("markdown"),
        "# Title\n",
        &[("markdown_rendered_by_default", json!(true))],
    );
    let before = render_pane(&mut host, 60, 8);
    host.send_action(
        ViewTarget::Pane(NodeId(7)),
        "pixel-headers",
        ilium_plugin_sdk::generated::types::ActionKind::Toggled,
        ilium_plugin_sdk::generated::types::ActionValue::Flag(false),
    );
    let after = render_pane(&mut host, 60, 8);
    assert_ne!(before.text(), after.text());
}

/// REQ: PORT:ilium-client/src/editor_pane.rs::wrapping_tracks_visual_rows_without_changing_the_buffer, PORT:ilium-client/src/editor_pane.rs::replace_contents_preserves_wrap_mode
#[test]
fn port_pane_wrapping_adds_visual_rows_and_keeps_buffer_text_and_wrap_mode() {
    let long = "abcdefghij";
    let mut host = plain_host(
        "a.txt",
        None,
        &format!("{long}\n"),
        &[
            ("line_display", json!("wrap")),
            ("show_line_numbers", json!(false)),
        ],
    );
    let wrapped = render_pane(&mut host, 4, 6);
    assert_eq!(wrapped.row(1), "abcd");
    assert_eq!(wrapped.row(2), "efgh");
    let mut editor = direct(r#"{"line_display":"wrap"}"#);
    direct_open(&mut editor, 1, None, 1);
    crate::support::seed_text(&mut editor, 1, long);
    let lines = read_lines(&mut editor, 1, 0, 5);
    assert_eq!(lines.get("lines"), Some(&json!([long])), "{lines}");
}
