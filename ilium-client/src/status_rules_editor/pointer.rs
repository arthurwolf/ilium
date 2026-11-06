//! Pointer handling for the rules editor's open condition or template list,
//! kept here so `mouse.rs` needs only a one-line hook.

use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Position;

use super::{picker, picker_area, EditorLayout};
use crate::app::App;

/// Handle a pointer event while a list is open. Returns `true` when the
/// event was consumed (any event while a list is open is: the list is
/// modal, so a click elsewhere only closes it).
pub(crate) fn picker_event(
    app: &mut App,
    kind: MouseEventKind,
    position: Position,
    regions: &EditorLayout,
) -> bool {
    let Some(mut editor) = app.custom_status_rules_editor.take() else {
        return false;
    };
    let Some(open) = editor.picker.as_mut() else {
        app.custom_status_rules_editor = Some(editor);
        return false;
    };
    let area = picker_area(regions);
    match kind {
        MouseEventKind::ScrollUp => open.move_by(-1),
        MouseEventKind::ScrollDown => open.move_by(1),
        MouseEventKind::Down(MouseButton::Left) => {
            if !area.contains(position) {
                editor.picker = None;
            } else if let Some(index) = picker::item_at(area, open, position) {
                open.highlighted = Some(index);
                super::keyboard::choose(app, &mut editor, regions.rules.height);
            }
        }
        _ => {}
    }
    app.custom_status_rules_editor = Some(editor);
    true
}
