//! The optional footer reserved at the *bottom* of a pane with an active
//! server-run progress monitor (see `ilium_core::PaneProgress`,
//! `ilium-server`'s `progress_monitor` module doc for how a value gets
//! there): a one-row percent gauge, followed by the monitor's freeform
//! status message.
//!
//! Placement is deliberately the opposite end of the pane from
//! `crate::last_prompt_banner`, which sits at the *top* (below the agent
//! toolbar) -- see `split_layout::PaneViewport::with_progress_reserved`.
//! The message is word-wrapped and middle-truncated the exact same way an
//! over-budget last prompt is, reusing
//! [`crate::last_prompt_banner::wrap_lines`]/
//! [`crate::last_prompt_banner::truncate_middle_rows`] rather than
//! duplicating that logic.

use ilium_core::PaneProgress;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{LineGauge, Paragraph};
use ratatui::Frame;

use crate::last_prompt_banner::{truncate_middle_rows, wrap_lines};
use crate::theme::{self, ColorScheme};

/// The exact number of rows the footer needs for `progress` at `width`
/// columns: 0 when there is no active progress, otherwise 1 (the percent
/// gauge) plus however many rows the message wraps/truncates to (0 when the
/// message is empty) -- never a fixed budget regardless of content, the
/// same discipline `last_prompt_banner::reserved_height` follows.
pub fn reserved_height(progress: Option<&PaneProgress>, width: u16, max_message_lines: u16) -> u16 {
    let Some(progress) = progress else {
        return 0;
    };
    let message_rows = if progress.message.is_empty() {
        0
    } else {
        truncate_middle_rows(wrap_lines(&progress.message, width), max_message_lines).len() as u16
    };
    1 + message_rows
}

/// Renders the progress footer into `area`: a background fill spanning the
/// whole reserved region (reusing [`theme::last_prompt_style`] so both
/// footers read as one consistent visual language), a one-row percent gauge
/// on the first row, and the wrapped/middle-truncated message on the rows
/// below. A `None` progress or a zero-height `area` renders nothing.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    progress: Option<&PaneProgress>,
    max_message_lines: u16,
    scheme: ColorScheme,
) {
    let Some(progress) = progress else {
        return;
    };
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("").style(theme::last_prompt_style(scheme)),
        area,
    );

    let [gauge_area, message_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .areas(area);

    // Defensive re-clamp: `ilium_core::Tree::set_pane_progress` already
    // clamps to `0.0..=100.0` before this ever reaches the tree, but
    // `LineGauge::ratio` panics outside `0.0..=1.0` -- a render path must
    // never be able to crash the whole TUI over a value that slipped past
    // an earlier guard some other way.
    let ratio = (f64::from(progress.percent) / 100.0).clamp(0.0, 1.0);
    let label = format!("{:.0}%", progress.percent.clamp(0.0, 100.0));
    // Distinct glyphs (solid vs. light shade) rather than same-character
    // filled/unfilled runs distinguished only by color/`Modifier::DIM` --
    // `DIM` renders inconsistently across terminals (often not at all), which
    // left the unfilled run looking identical to the filled one in practice.
    let gauge = LineGauge::default()
        .ratio(ratio)
        .label(label)
        .filled_symbol(symbols::shade::FULL)
        .unfilled_symbol(symbols::shade::LIGHT)
        .filled_style(
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD),
        )
        .unfilled_style(Style::new().fg(theme::muted_accent_bg(scheme)));
    frame.render_widget(gauge, gauge_area);

    if !progress.message.is_empty() {
        let lines: Vec<Line> = truncate_middle_rows(
            wrap_lines(&progress.message, message_area.width),
            max_message_lines,
        )
        .into_iter()
        .map(|(line, is_seam)| {
            if is_seam {
                Line::from(Span::styled(line, Style::new().add_modifier(Modifier::DIM)))
            } else {
                Line::from(line)
            }
        })
        .collect();
        frame.render_widget(Paragraph::new(lines), message_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(percent: f32, message: &str) -> PaneProgress {
        PaneProgress {
            percent,
            message: message.to_string(),
        }
    }

    #[test]
    fn no_progress_reserves_nothing() {
        assert_eq!(reserved_height(None, 80, 4), 0);
    }

    #[test]
    fn a_bare_percent_with_no_message_reserves_only_the_gauge_row() {
        assert_eq!(reserved_height(Some(&progress(50.0, "")), 80, 4), 1);
    }

    #[test]
    fn a_short_message_reserves_the_gauge_plus_one_row() {
        assert_eq!(
            reserved_height(Some(&progress(50.0, "frame 10/100")), 80, 4),
            2
        );
    }

    #[test]
    fn a_long_message_is_capped_at_the_gauge_plus_max_message_lines() {
        let message = "one two three four five six seven eight nine ten eleven twelve";
        assert_eq!(reserved_height(Some(&progress(50.0, message)), 4, 3), 4);
    }

    /// The filled and unfilled runs of the gauge must be told apart by glyph
    /// (solid block vs. light shade), not merely by color/`Modifier::DIM` --
    /// `DIM` renders inconsistently across terminals, which previously left
    /// both runs using the same `─` character and reading as one solid,
    /// undifferentiated line.
    #[test]
    fn the_gauge_row_uses_a_solid_glyph_up_to_the_ratio_and_a_light_glyph_after() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut terminal = Terminal::new(TestBackend::new(24, 1)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    Some(&progress(50.0, "")),
                    2,
                    ColorScheme::Dark,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // "50% " occupies the first 4 columns, leaving 20 for the bar itself
        // -- half filled, half not, at a 50% ratio.
        let bar_symbols: String = (4..24).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(
            bar_symbols,
            format!(
                "{}{}",
                symbols::shade::FULL.repeat(10),
                symbols::shade::LIGHT.repeat(10)
            )
        );
        assert_eq!(buf[(4, 0)].fg, theme::accent_bg());
        assert_eq!(buf[(23, 0)].fg, theme::muted_accent_bg(ColorScheme::Dark));
    }
}
