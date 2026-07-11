//! Renders the leader-key reference (`keymap::LEADER_BINDINGS`) as a
//! centered popup overlay, so the bindings can never drift out of sync
//! with what `app.rs` actually dispatches.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::keymap::LEADER_BINDINGS;
use crate::layout::centered_rect;
use crate::theme;

/// Draws the help popup, centered within `area` at roughly 60% width /
/// 70% height.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);

    // Clear the popup's own footprint first so it reads as an opaque
    // panel rather than a see-through overlay on whatever was drawn
    // underneath it this frame.
    frame.render_widget(Clear, popup_area);

    let mut lines = Vec::with_capacity(LEADER_BINDINGS.len() + 4);
    lines.push(Line::from(Span::styled(
        "illium — keyboard reference",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for binding in LEADER_BINDINGS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("Ctrl+A then {}", binding.letter),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  —  "),
            Span::raw(binding.description),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Mouse",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(
        "left-click a pane to focus it; click a group to expand/collapse",
    ));
    lines.push(Line::from(
        "drag a tree entry onto a group or after a pane to move it",
    ));
    lines.push(Line::from(
        "right-click a tree entry for rename, move, create, and close actions",
    ));
    lines.push(Line::from(
        "hover the tree footer for the shell / Claude / Codex / editor / group buttons",
    ));
    lines.push(Line::from(
        "hover a tree row for ✎/↑/↓ controls; ✎ renames, pane arrows cross into adjacent groups at boundaries",
    ));
    lines.push(Line::from(
        "click the terminal to focus it; mouse-aware terminal apps receive their mouse input",
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Prompts",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(
        "rename and run-command open a centered input popup, not the status bar",
    ));
    lines.push(Line::from(
        "closing a group with items, or an editor with unsaved changes, asks y/n first",
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "press Ctrl+A ? again, or Esc, to close",
        Style::new().add_modifier(Modifier::ITALIC),
    )));

    let block = theme::block(true).title(theme::chrome_title("Help"));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}
