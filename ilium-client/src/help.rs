//! Renders the live App-owned leader map as a centered popup overlay. The
//! same slice is dispatched by `keys`, so applying a setting immediately
//! changes both input and this reference.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::keymap;
use crate::layout::centered_rect;
use crate::theme;

/// Draws the complete live key reference in two compact columns. Keeping the
/// action table under one terminal page means a newly added action never
/// disappears below a non-scrollable modal's fold.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    shortcut_base: keymap::ShortcutBase,
    navigation_shortcut_base: keymap::ShortcutBase,
    bindings: &[keymap::KeyBinding],
) {
    let popup_area = centered_rect(94, 94, area);

    // Clear the popup's own footprint first so it reads as an opaque
    // panel rather than a see-through overlay on whatever was drawn
    // underneath it this frame.
    frame.render_widget(Clear, popup_area);

    let mut lines = Vec::with_capacity(bindings.len().div_ceil(2) + 6);
    lines.push(Line::from(Span::styled(
        "ilium — keyboard reference",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for pair in bindings.chunks(2) {
        let mut spans = Vec::new();
        for (index, binding) in pair.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(
                format!(
                    "{} {}",
                    keymap::action_prefix_label(
                        binding.action,
                        shortcut_base,
                        navigation_shortcut_base,
                    ),
                    keymap::key_label(binding.key)
                ),
                Style::new().add_modifier(Modifier::BOLD),
            ));
            // A 22-cell description keeps two complete action columns inside
            // the 80-column terminal baseline, while the Keyboard settings
            // table exposes the full action name for remapping.
            let summary = binding.description.chars().take(22).collect::<String>();
            spans.push(Span::raw(format!(" — {summary:<22}")));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Mouse: pane focus · tree expand/reorder · context menus · tree footer Settings · {} rename",
        theme::PEN_ICON,
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "press {} {} again, or Esc, to close",
            shortcut_base.label(),
            bindings
                .iter()
                .find(|binding| binding.action == keymap::Action::Help)
                .map(|binding| keymap::key_label(binding.key))
                .unwrap_or_else(|| "?".to_string()),
        ),
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
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    keymap::ShortcutBase::B,
                    keymap::DEFAULT_NAVIGATION_SHORTCUT_BASE,
                    keymap::LEADER_BINDINGS,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Ctrl+B ?"));
        assert!(rendered.contains("press Ctrl+B ? again"));
        assert!(rendered.contains(theme::PEN_ICON));
        assert!(!rendered.contains("Ctrl+A then ?"));
    }

    #[test]
    fn help_renders_a_live_remapped_action_table() {
        let mut bindings = keymap::LEADER_BINDINGS.to_vec();
        keymap::assign_key(
            &mut bindings,
            keymap::Action::NewGroup,
            keymap::BindingKey::Character('z'),
        )
        .expect("z is free in the default map");
        let backend = TestBackend::new(140, 60);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    keymap::ShortcutBase::A,
                    keymap::DEFAULT_NAVIGATION_SHORTCUT_BASE,
                    &bindings,
                )
            })
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Ctrl+A z — New group"));
        assert!(!rendered.contains("Ctrl+A g — New group"));
    }
}
