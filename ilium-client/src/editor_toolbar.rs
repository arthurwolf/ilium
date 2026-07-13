//! The always-visible toolbar rendered above every editor pane's
//! content: a Source/Rendered view-mode toggle (markdown files only)
//! plus independent Line-numbers and Minimap toggles. Mirrors
//! `tree_ui`'s toolbar pattern -- a pure `button_rects` shared by
//! rendering and click hit-testing, so the two can never drift apart.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::editor_pane::{EditorPane, EditorViewMode};

/// One toolbar click target. `app.rs::execute_editor_toolbar_action`
/// applies the effect; this module only knows how to draw and hit-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    ViewSource,
    ViewRendered,
    ToggleLineNumbers,
    ToggleMinimap,
    ToggleAutosave,
    Save,
    SaveAs,
}

struct Button {
    action: ToolbarAction,
    label: String,
    /// Highlighted as the pane's current setting (not a hover state --
    /// this toolbar has no hover-only affordances, unlike the tree's).
    active: bool,
}

/// The buttons to show for `pane`, in display order. The Source/Rendered
/// pair only appears for markdown files -- toggling view mode is a no-op
/// for anything else, so the control doesn't exist for it.
fn buttons_for(pane: &EditorPane) -> Vec<Button> {
    let mut buttons = Vec::new();
    if pane.is_markdown() {
        buttons.push(Button {
            action: ToolbarAction::ViewSource,
            label: " Source ".to_string(),
            active: pane.view_mode == EditorViewMode::Source,
        });
        buttons.push(Button {
            action: ToolbarAction::ViewRendered,
            label: " Rendered ".to_string(),
            active: pane.view_mode == EditorViewMode::Rendered,
        });
    }
    buttons.push(Button {
        action: ToolbarAction::ToggleLineNumbers,
        label: format!(
            " # Lines: {} ",
            if pane.show_line_numbers { "on" } else { "off" }
        ),
        active: pane.show_line_numbers,
    });
    buttons.push(Button {
        action: ToolbarAction::ToggleMinimap,
        label: format!(
            " \u{25a4} Map: {} ",
            if pane.show_minimap { "on" } else { "off" }
        ),
        active: pane.show_minimap,
    });
    buttons.push(Button {
        action: ToolbarAction::ToggleAutosave,
        label: format!(
            " \u{21bb} Auto: {} ",
            if pane.show_autosave { "on" } else { "off" }
        ),
        active: pane.show_autosave,
    });
    buttons
}

/// The buttons anchored to the toolbar's right edge, in display order
/// left-to-right (rightmost last) -- these apply to every editor pane
/// regardless of file type, unlike `buttons_for`'s markdown-only pair.
fn right_buttons_for(pane: &EditorPane) -> Vec<Button> {
    vec![
        Button {
            action: ToolbarAction::Save,
            label: " Save ".to_string(),
            // Highlighted to draw the eye whenever there's something to
            // save, the same "active means needs attention" language the
            // on/off toggles above use for their current setting.
            active: pane.dirty,
        },
        Button {
            action: ToolbarAction::SaveAs,
            label: " Save As\u{2026} ".to_string(),
            active: false,
        },
    ]
}

/// Each button's exact screen rect, clipped to `area` -- shared by
/// `render` and `action_at` so a click always maps to what's actually
/// drawn. Left-aligned view/toggle buttons come first, followed by the
/// Save/Save As pair anchored to the right edge; on a terminal too narrow
/// for both groups, right-edge buttons are dropped first (rather than
/// overlapping the left group) since the toggles are still reachable via
/// their leader-key bindings but Save/Save As currently aren't.
fn button_rects(area: Rect, pane: &EditorPane) -> Vec<(ToolbarAction, Rect, String, bool)> {
    let mut x = area.x;
    let mut rects = Vec::new();
    for button in buttons_for(pane) {
        let width = button.label.chars().count() as u16;
        if x + width > area.right() {
            break;
        }
        rects.push((
            button.action,
            Rect::new(x, area.y, width, 1),
            button.label,
            button.active,
        ));
        x += width + 1; // one-column gap between buttons
    }
    let left_edge = x;

    // Walked in reverse (rightmost button first) since that's what anchors
    // the *last* entry of `right_buttons_for` (Save As) to the right edge,
    // with earlier ones (Save) laid out to its left -- then reversed back
    // before appending, so `rects`'s overall order still reads left to
    // right like `buttons_for`'s half does.
    let mut right_x = area.right();
    let mut right_rects = Vec::new();
    for button in right_buttons_for(pane).into_iter().rev() {
        let width = button.label.chars().count() as u16;
        if right_x < left_edge + width {
            break;
        }
        right_x -= width;
        right_rects.push((
            button.action,
            Rect::new(right_x, area.y, width, 1),
            button.label,
            button.active,
        ));
        right_x = right_x.saturating_sub(1); // one-column gap between buttons
    }
    right_rects.reverse();
    rects.extend(right_rects);
    rects
}

/// Returns the toolbar action at a terminal coordinate, if any.
pub fn action_at(area: Rect, pane: &EditorPane, position: Position) -> Option<ToolbarAction> {
    if !area.contains(position) {
        return None;
    }
    button_rects(area, pane)
        .into_iter()
        .find(|(_, rect, _, _)| rect.contains(position))
        .map(|(action, ..)| action)
}

/// Draws the toolbar into `area` (one row, reserved above every editor
/// pane's content -- see `editor_chrome::compute`).
pub fn render(frame: &mut Frame, area: Rect, pane: &EditorPane) {
    for (_, rect, label, active) in button_rects(area, pane) {
        let style = if active {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))), rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn markdown_pane() -> EditorPane {
        let mut pane = EditorPane::empty();
        pane.path = Some(PathBuf::from("notes.md"));
        pane
    }

    #[test]
    fn markdown_pane_shows_view_mode_buttons() {
        let pane = markdown_pane();
        let area = Rect::new(0, 0, 80, 1);
        assert_eq!(
            action_at(area, &pane, Position::new(2, 0)),
            Some(ToolbarAction::ViewSource)
        );
    }

    #[test]
    fn plain_text_pane_has_no_view_mode_buttons() {
        let mut pane = EditorPane::empty();
        pane.path = Some(PathBuf::from("notes.txt"));
        let area = Rect::new(0, 0, 80, 1);
        // First button should be the line-numbers toggle, not Source.
        assert_eq!(
            action_at(area, &pane, Position::new(2, 0)),
            Some(ToolbarAction::ToggleLineNumbers)
        );
    }

    #[test]
    fn click_outside_toolbar_row_misses() {
        let pane = markdown_pane();
        let area = Rect::new(0, 5, 80, 1);
        assert_eq!(action_at(area, &pane, Position::new(2, 0)), None);
    }

    #[test]
    fn save_and_save_as_are_anchored_to_the_right_edge() {
        let pane = markdown_pane();
        let area = Rect::new(0, 0, 80, 1);
        let rects = button_rects(area, &pane);
        let (save_action, save_rect, ..) = rects[rects.len() - 2];
        let (save_as_action, save_as_rect, ..) = rects[rects.len() - 1];
        assert_eq!(save_action, ToolbarAction::Save);
        assert_eq!(save_as_action, ToolbarAction::SaveAs);
        assert!(save_rect.x < save_as_rect.x);
        assert_eq!(save_as_rect.right(), area.right());
    }

    #[test]
    fn dirty_pane_highlights_the_save_button() {
        // `action_at` only ever returns a `ToolbarAction`, never the
        // per-button `active` flag, so it can't observe highlighting at
        // all -- go through `button_rects` (which carries `active`)
        // instead, and check both states so a hard-coded `true` can't
        // slip back in unnoticed.
        let mut pane = markdown_pane();
        let area = Rect::new(0, 0, 80, 1);

        let save_button_active = |pane: &EditorPane| {
            button_rects(area, pane)
                .into_iter()
                .find(|(action, ..)| *action == ToolbarAction::Save)
                .map(|(_, _, _, active)| active)
                .expect("Save button should be present")
        };

        assert!(!pane.dirty);
        assert!(!save_button_active(&pane));

        pane.dirty = true;
        assert!(save_button_active(&pane));
    }

    #[test]
    fn narrow_toolbar_drops_right_buttons_before_overlapping_the_left_group() {
        let pane = markdown_pane();
        let area = Rect::new(0, 0, 10, 1);
        let rects = button_rects(area, &pane);
        assert!(!rects
            .iter()
            .any(|(action, ..)| matches!(action, ToolbarAction::Save | ToolbarAction::SaveAs)));
    }
}
