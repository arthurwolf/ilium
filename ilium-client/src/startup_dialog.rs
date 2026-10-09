//! The start-up dialog: a small centred box with three lines -- the general
//! category of work, the item being worked on, and a progress bar -- so a
//! waiting user can see that ilium is busy and not frozen.
//!
//! It is drawn in two places that share one layout. Before the render loop
//! exists (settings, audio devices, connecting) [`StartupDialog`] writes it
//! straight to the terminal; once the loop runs, `ui` paints the same lines
//! over the interface until the server's initial state has arrived.

use std::io::Write;

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_WIDTH: u16 = 64;
const MIN_WIDTH: u16 = 24;
/// Border, padding, category, item, bar, padding, border.
const HEIGHT: u16 = 7;

/// What the dialog says. `fraction == None` draws a moving bar for work whose
/// size is not known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DialogText {
    pub category: String,
    pub item: String,
    pub fraction: Option<f64>,
}

/// The dialog rectangle centred in `area`, or `None` when it cannot fit.
pub(crate) fn dialog_area(area: Rect) -> Option<Rect> {
    if area.width < MIN_WIDTH || area.height < HEIGHT {
        return None;
    }
    let width = MAX_WIDTH.min(area.width.saturating_sub(4)).max(MIN_WIDTH);
    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - HEIGHT) / 2,
        width,
        HEIGHT,
    ))
}

fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut fitted = String::new();
    let mut used = 0;
    for character in text.chars() {
        let advance = character.width().unwrap_or(0);
        if used + advance + 1 > width {
            break;
        }
        fitted.push(character);
        used += advance;
    }
    fitted.push('…');
    fitted
}

fn padded(text: &str, width: usize) -> String {
    let fitted = fit(text, width);
    let gap = width.saturating_sub(fitted.width());
    format!("{fitted}{}", " ".repeat(gap))
}

fn bar(width: usize, fraction: Option<f64>, tick: u64) -> String {
    let cells = width.saturating_sub(5).max(4);
    let (filled, label) = match fraction {
        Some(fraction) => {
            let filled = (fraction * cells as f64).round() as usize;
            (
                (0..cells).map(|cell| cell < filled).collect::<Vec<_>>(),
                format!(" {:>3}%", (fraction * 100.0).round() as u32),
            )
        }
        None => {
            // A short block sweeps back and forth across the bar.
            let block = (cells / 5).max(2);
            let span = cells - block;
            let step = (tick as usize) % (2 * span.max(1));
            let start = if step <= span { step } else { 2 * span - step };
            (
                (0..cells)
                    .map(|cell| cell >= start && cell < start + block)
                    .collect(),
                "     ".to_owned(),
            )
        }
    };
    let cells: String = filled
        .into_iter()
        .map(|on| if on { '█' } else { '░' })
        .collect();
    format!("{cells}{label}")
}

/// The complete dialog, one string per row, each exactly `width` columns.
pub(crate) fn dialog_lines(width: u16, text: &DialogText, tick: u64) -> Vec<String> {
    let inner = usize::from(width).saturating_sub(4);
    let horizontal_width = usize::from(width).saturating_sub(2);
    let title = crate::theme::chrome_title("Ilium").to_string();
    let horizontal =
        "─".repeat(horizontal_width.saturating_sub(UnicodeWidthStr::width(title.as_str()) + 1));
    let bottom_horizontal = "─".repeat(horizontal_width);
    let row = |content: &str| format!("│ {} │", padded(content, inner));
    vec![
        format!("╭{title} {horizontal}╮"),
        row(""),
        row(&text.category),
        row(&text.item),
        row(&bar(inner, text.fraction, tick)),
        row(""),
        format!("╰{bottom_horizontal}╯"),
    ]
}

/// Paints the dialog before the render loop exists. The presenter has not
/// emitted a frame yet, so nothing else is writing to the terminal.
pub(crate) struct StartupDialog {
    tick: u64,
}

impl StartupDialog {
    pub(crate) fn new() -> Self {
        Self { tick: 0 }
    }

    pub(crate) fn show(&mut self, category: &str, item: &str, fraction: Option<f64>) {
        self.tick = self.tick.wrapping_add(1);
        let Ok((columns, rows)) = crossterm::terminal::size() else {
            return;
        };
        let Some(area) = dialog_area(Rect::new(0, 0, columns, rows)) else {
            return;
        };
        let text = DialogText {
            category: category.to_owned(),
            item: item.to_owned(),
            fraction,
        };
        let mut out = std::io::stdout().lock();
        for (offset, line) in dialog_lines(area.width, &text, self.tick)
            .iter()
            .enumerate()
        {
            let _ = queue!(out, MoveTo(area.x, area.y + offset as u16));
            let _ = match offset {
                2 => queue!(
                    out,
                    SetAttribute(Attribute::Bold),
                    Print(line),
                    SetAttribute(Attribute::Reset)
                ),
                3 => queue!(
                    out,
                    SetForegroundColor(Color::Grey),
                    Print(line),
                    ResetColor
                ),
                4 => queue!(
                    out,
                    SetForegroundColor(Color::Cyan),
                    Print(line),
                    ResetColor
                ),
                _ => queue!(out, Print(line)),
            };
        }
        let _ = out.flush();
    }

    /// Shows what the server reports while it restores its session, or a
    /// moving bar while it has not said anything yet.
    pub(crate) fn show_server_progress(&mut self, progress: Option<&ilium_ipc::StartupProgress>) {
        match progress {
            Some(progress) => self.show(&progress.category, &progress.item, progress.fraction()),
            None => self.show("Starting ilium", "Connecting to the session server", None),
        }
    }

    /// Wipes the screen so the first interface frame starts from blank cells.
    pub(crate) fn clear(&mut self) {
        let mut out = std::io::stdout().lock();
        let _ = queue!(out, ResetColor, Clear(ClearType::All), MoveTo(0, 0));
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(fraction: Option<f64>) -> DialogText {
        DialogText {
            category: "Restoring panes".into(),
            item: "A very long pane title that cannot possibly fit inside the dialog box width"
                .into(),
            fraction,
        }
    }

    #[test]
    fn every_row_is_exactly_the_dialog_width() {
        for width in [MIN_WIDTH, 40, MAX_WIDTH] {
            for fraction in [None, Some(0.0), Some(0.42), Some(1.0)] {
                for line in dialog_lines(width, &text(fraction), 7) {
                    assert_eq!(line.width(), usize::from(width), "{line:?}");
                }
            }
        }
    }

    #[test]
    fn top_border_uses_shared_ilium_chrome_title() {
        for width in [MIN_WIDTH, 40, MAX_WIDTH] {
            let line = dialog_lines(width, &text(Some(0.5)), 0).remove(0);
            let title = crate::theme::chrome_title("Ilium").to_string();
            assert!(line.starts_with(&format!("╭{title} ─")), "{line:?}");
            assert!(line.ends_with('╮'));
            assert_eq!(line.width(), usize::from(width));
        }
    }

    #[test]
    fn known_progress_fills_proportionally_and_labels_percent() {
        let line = dialog_lines(MAX_WIDTH, &text(Some(0.5)), 0)[4].clone();
        assert!(line.contains(" 50%"));
        let filled = line.chars().filter(|c| *c == '█').count();
        let empty = line.chars().filter(|c| *c == '░').count();
        assert!(filled > 0 && empty > 0 && filled.abs_diff(empty) <= 1);
    }

    #[test]
    fn unknown_progress_moves_between_ticks() {
        let first = dialog_lines(MAX_WIDTH, &text(None), 0)[4].clone();
        let later = dialog_lines(MAX_WIDTH, &text(None), 9)[4].clone();
        assert_ne!(first, later);
    }

    #[test]
    fn small_terminals_get_no_dialog_and_large_ones_get_it_centred() {
        assert!(dialog_area(Rect::new(0, 0, 20, 30)).is_none());
        assert!(dialog_area(Rect::new(0, 0, 100, 5)).is_none());
        let area = dialog_area(Rect::new(0, 0, 100, 30)).unwrap();
        assert_eq!((area.width, area.height), (MAX_WIDTH, HEIGHT));
        assert_eq!(area.x, (100 - MAX_WIDTH) / 2);
    }

    #[test]
    fn composed_startup_progress_overlay_is_visible_at_terminal_sizes() {
        use ratatui::{backend::TestBackend, Terminal};

        let workspace = tempfile::tempdir().unwrap();
        for (width, height) in [(24, 10), (40, 12), (80, 24), (120, 40)] {
            let area = Rect::new(0, 0, width, height);
            let mut app = crate::app::App::new(
                "synthetic-startup-progress".into(),
                workspace.path().to_path_buf(),
            );
            // A path makes the real startup overlay visible. No file means
            // the renderer shows its honest indeterminate startup message.
            app.startup_progress_path = Some(workspace.path().join("session.startup"));
            app.set_screen_area(area);

            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();

            let buffer = terminal.backend().buffer();
            let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(rendered.contains("Ilium"), "title at {width}x{height}");
            assert!(
                rendered.contains("Loading the session"),
                "startup category at {width}x{height}"
            );
            assert!(
                rendered.contains("Receiving panes"),
                "startup detail at {width}x{height}"
            );
            assert!(rendered.contains('░'), "progress bar at {width}x{height}");
            crate::ui_capture::save(&format!("startup-progress-{width}x{height}"), &terminal);
        }
    }
}
