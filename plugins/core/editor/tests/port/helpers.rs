//! Small helpers shared by the port twins.

use crate::support::{active_host, open_file, render_pane};
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::{
    BufferId, EditorPointerEvent, Event, Intent, NodeId, PointerButton, PointerKind,
    PointerModifiers, TextPosition, ViewPointer,
};
use ilium_plugin_sdk::testing::host::{FakeHost, Screen};
use serde_json::json;

/// A left-button press on `line`/`column` of buffer 1 in pane 7.
pub fn click_event(line: u32, column: u32) -> Event {
    Event::EditorPointer(EditorPointerEvent {
        buffer: BufferId(1),
        pane: NodeId(7),
        position: TextPosition { line, column },
        pointer: ViewPointer {
            kind: PointerKind::Down,
            button: PointerButton::Left,
            modifiers: PointerModifiers(0),
            column: 0,
            row: 0,
            sub_x: 128,
            sub_y: 128,
            clicks: 1,
        },
    })
}

/// A host with the minimap off and `text` open as `file` (language from the caller).
pub fn plain_host(
    file: &str,
    language: Option<&str>,
    text: &str,
    extra: &[(&str, serde_json::Value)],
) -> FakeHost {
    let mut settings: Vec<(&str, serde_json::Value)> = vec![("show_minimap", json!(false))];
    settings.extend(extra.iter().cloned());
    let mut host = active_host(&settings);
    open_file(&mut host, 1, file, language, text);
    host
}

/// A host showing `text` in the rendered Markdown view.
pub fn rendered_host(text: &str) -> FakeHost {
    plain_host(
        "notes.md",
        Some("markdown"),
        text,
        &[("markdown_rendered_by_default", json!(true))],
    )
}

/// Renders the rendered-Markdown view of `text` at 60x16.
pub fn rendered_screen(text: &str) -> Screen {
    let mut host = rendered_host(text);
    render_pane(&mut host, 60, 16)
}

/// Row of the first occurrence of `needle`; the test fails when it is absent.
pub fn row_of(screen: &Screen, needle: &str) -> i32 {
    match screen.find(needle) {
        Some((row, _)) => i32::from(row),
        None => panic!("{needle:?} not on screen:\n{}", screen.text()),
    }
}

/// The edit a checkbox click submits as (start line, start column, end line, end column, text).
pub fn single_edit(submissions: &[Submission]) -> Option<(u32, u32, u32, u32, String)> {
    let first = submissions.first()?;
    let Intent::EditorApplyEdits(spec) = &first.intent else {
        return None;
    };
    let edit = spec.edits.first()?;
    Some((
        edit.range.start.line,
        edit.range.start.column,
        edit.range.end.line,
        edit.range.end.column,
        edit.replacement.clone(),
    ))
}
