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

/// Non-table lines every render always includes: the title, a blank
/// separator, the blank line after the table, the two mouse/terminal info
/// lines, and the close hint. This crate's own `Paragraph` never wraps or
/// scrolls (see `render`), so this count is the fixed budget subtracted from
/// the popup's interior height before any table rows are laid out.
const FIXED_FOOTER_LINE_COUNT: usize = 6;

/// Draws the complete live key reference in two compact columns. The table
/// is capped to however many rows actually fit above the info footer, so a
/// terminal too short (or an action table that outgrew the popup) truncates
/// the *table*, never the footer -- the close hint must always be visible,
/// since a non-scrollable modal with no visible way to dismiss it would trap
/// the user.
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

    let table_rows: Vec<Line> = bindings
        .chunks(2)
        .map(|pair| {
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
            Line::from(spans)
        })
        .collect();

    // The block reserves exactly the top and bottom border rows; the title
    // shares the top border row rather than adding one of its own.
    let interior_height = usize::from(popup_area.height.saturating_sub(2));
    let rows_budget = interior_height.saturating_sub(FIXED_FOOTER_LINE_COUNT);
    let table_will_truncate = table_rows.len() > rows_budget;
    // Truncation needs its own notice row carved out of the same budget so
    // the notice itself can never be what pushes the footer off-screen. A
    // zero-row budget has no row to carve, so on a terminal that short the
    // notice is dropped too -- the footer (and its close hint) keeps
    // priority over everything table-related.
    let show_truncation_notice = table_will_truncate && rows_budget > 0;
    let visible_row_count = if table_will_truncate {
        rows_budget.saturating_sub(1)
    } else {
        rows_budget
    }
    .min(table_rows.len());
    let visible_action_count: usize = bindings
        .chunks(2)
        .take(visible_row_count)
        .map(|pair| pair.len())
        .sum();
    let hidden_action_count = bindings.len() - visible_action_count;

    let mut lines = Vec::with_capacity(interior_height);
    lines.push(Line::from(Span::styled(
        "ilium — keyboard reference",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.extend(table_rows.into_iter().take(visible_row_count));
    if show_truncation_notice {
        lines.push(Line::from(format!(
            "… {hidden_action_count} more actions — see Settings → Keyboard for the full list",
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Mouse: pane focus · tree expand/reorder · context menus · tree footer Settings · {} rename",
        theme::PEN_ICON,
    )));
    lines.push(Line::from(
        "Terminal history: wheel / Shift+PgUp/PgDn · Shift+End live · Ctrl+End app",
    ));
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
        assert!(rendered.contains("Shift+End live · Ctrl+End app"));
        // Regressing to the pre-"live remap" wording would still contain
        // "Ctrl+B", so this must check the actual old phrase, not a base
        // letter that this test doesn't even select.
        assert!(!rendered.contains("Ctrl+B then ?"));
    }

    /// The 80x24 baseline this file's own comments claim as the target: the
    /// close hint (the only way a user learns how to dismiss a
    /// non-scrollable modal) must survive even though `LEADER_BINDINGS` has
    /// long since outgrown two columns' worth of rows at that height.
    #[test]
    fn help_always_shows_the_close_hint_at_the_baseline_terminal_size() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    keymap::ShortcutBase::A,
                    keymap::DEFAULT_NAVIGATION_SHORTCUT_BASE,
                    keymap::LEADER_BINDINGS,
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
        assert!(rendered.contains("again, or Esc, to close"));
        assert!(rendered.contains("Shift+End live · Ctrl+End app"));
        assert!(rendered.contains("more actions — see Settings"));
    }

    /// On terminals so short the popup interior only fits the fixed footer
    /// (a zero table-row budget), the truncation notice must be dropped
    /// rather than pushing the close hint off-screen. Sweeps a range of tiny
    /// heights so the zero-budget geometry is hit regardless of how the
    /// percentage layout rounds the popup's height.
    #[test]
    fn help_keeps_the_close_hint_even_on_a_tiny_terminal() {
        for height in 9..=14 {
            let backend = TestBackend::new(80, height);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        frame.area(),
                        keymap::ShortcutBase::A,
                        keymap::DEFAULT_NAVIGATION_SHORTCUT_BASE,
                        keymap::LEADER_BINDINGS,
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
            assert!(
                rendered.contains("again, or Esc, to close"),
                "close hint clipped at terminal height {height}",
            );
        }
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
