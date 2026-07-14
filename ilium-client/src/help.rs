//! Renders the leader-key reference (`keymap::effective_bindings`, the
//! possibly user-remapped table) as a centered popup overlay, so the
//! bindings shown here can never drift out of sync with what `app.rs`
//! actually dispatches (`keymap::action_for` searches the same table).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::keymap;
use crate::layout::centered_rect;
use crate::theme;

/// Draws the help popup, centered within `area` at roughly 60% width /
/// 70% height.
pub fn render(frame: &mut Frame, area: Rect, shortcut_base: keymap::ShortcutBase) {
    let popup_area = centered_rect(60, 70, area);

    // Clear the popup's own footprint first so it reads as an opaque
    // panel rather than a see-through overlay on whatever was drawn
    // underneath it this frame.
    frame.render_widget(Clear, popup_area);

    let bindings = keymap::effective_bindings();
    let mut lines = Vec::with_capacity(bindings.len() + 4);
    lines.push(Line::from(Span::styled(
        "ilium — keyboard reference",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for binding in bindings {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} then {}", shortcut_base.label(), binding.letter),
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
        "use the right-aligned tree-footer gear for Settings (appearance and keyboard)",
    ));
    lines.push(Line::from(
        "focus or hover the tree panel to reveal its footer actions",
    ));
    lines.push(Line::from(format!(
        "hover a tree row for {}/↑/↓ controls; {} renames, pane arrows cross into adjacent groups at boundaries",
        theme::PEN_ICON,
        theme::PEN_ICON,
    )));
    lines.push(Line::from(
        "click the terminal to focus it; mouse-aware terminal apps receive their mouse input",
    ));
    lines.push(Line::from(
        "scroll the wheel over a terminal pane to browse its scrollback (Shift+PageUp/PageDown also works); typing returns to the live view",
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
        format!("press {} ? again, or Esc, to close", shortcut_base.label()),
        Style::new().add_modifier(Modifier::ITALIC),
    )));

    let block = theme::block(true).title(theme::chrome_title("Help"));
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn help_renders_the_current_shortcut_base_in_rows_and_close_hint() {
        let backend = TestBackend::new(140, 60);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), keymap::ShortcutBase::B))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Ctrl+B then ?"));
        assert!(rendered.contains("press Ctrl+B ? again"));
        assert!(rendered.contains(theme::PEN_ICON));
        assert!(!rendered.contains("Ctrl+A then ?"));
    }
}
