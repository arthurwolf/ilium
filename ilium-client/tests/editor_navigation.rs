//! Integration coverage for editor source navigation without the full TUI.
//!
//! `EditorPane` is the boundary that keeps the native `TextArea` viewport
//! and the syntax-highlighted renderer's explicit viewport in sync, so this
//! exercises the wheel-scroll contract independently of terminal setup.

use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ilium_client::app::{App, PaneRuntime, RightPanelTarget};
use ilium_client::editor_pane::EditorPane;
use ilium_client::layout::UiLayout;
use ilium_core::{PaneContentKind, ROOT_ID};
use ratatui::layout::{Position, Rect};
use ratatui_textarea::TextArea;

/// Owns a scratch fixture file in the OS temp dir for the duration of a
/// test. Cleanup lives in `Drop` rather than a trailing `fs::remove_file`
/// call so a mid-test `assert_eq!` panic still removes the file instead of
/// leaking it on disk on every failed run.
struct EditorFixture {
    path: PathBuf,
}

impl EditorFixture {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ilium-editor-navigation-{}-{}.txt",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&path, "seed\n").expect("write editor fixture");
        Self { path }
    }
}

impl std::ops::Deref for EditorFixture {
    type Target = PathBuf;

    fn deref(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for EditorFixture {
    fn drop(&mut self) {
        // Best-effort: ignore errors so cleanup never masks (or panics
        // during) the real test failure/unwind.
        let _ = fs::remove_file(&self.path);
    }
}

#[test]
fn wheel_navigation_moves_the_source_viewport_and_keeps_it_until_cursor_navigation() {
    let fixture = EditorFixture::create();
    let mut editor = EditorPane::load(fixture.clone()).expect("load editor fixture");
    editor.textarea = TextArea::from((0..10).map(|row| format!("line {row}")));

    editor.scroll_source_view(3, 4);
    editor.update_source_scroll_mirror(4);
    assert_eq!(editor.source_scroll_row(), 3);

    editor.scroll_source_view(100, 4);
    assert_eq!(editor.source_scroll_row(), 6);

    editor.jump_to_line(0);
    editor.update_source_scroll_mirror(4);
    assert_eq!(editor.source_scroll_row(), 0);
}

#[test]
fn source_pane_wheel_event_routes_to_the_editor_viewport() {
    let fixture = EditorFixture::create();
    let mut app = App::new("test".to_string(), std::env::temp_dir());
    let group_id = app.tree.add_group(ROOT_ID, "editors").expect("add group");
    let pane_id = app
        .tree
        .add_pane(group_id, "notes.txt", PaneContentKind::Editor)
        .expect("add editor pane");
    let mut editor = EditorPane::load(fixture.clone()).expect("load editor fixture");
    editor.textarea = TextArea::from((0..20).map(|row| format!("line {row}")));
    app.panes
        .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));
    app.right_panel_target = RightPanelTarget::Pane { pane_id };
    // Keep the body shorter than the buffer so the wheel has a real scroll
    // range; a full-height viewport correctly clamps movement to zero.
    app.layout = UiLayout::from_screen_area(Rect::new(0, 0, 120, 8));

    let content_area = app.editor_content_area(pane_id);
    assert!(app
        .layout
        .pane_content_area
        .contains(Position::new(content_area.x, content_area.y)));
    app.handle_pane_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: content_area.x,
            row: content_area.y,
            modifiers: KeyModifiers::empty(),
        },
        Position::new(content_area.x, content_area.y),
    );

    let Some(PaneRuntime::Editor(editor)) = app.panes.get(&pane_id) else {
        panic!("editor pane should remain present");
    };
    assert_eq!(editor.source_scroll_row(), 3);
}
