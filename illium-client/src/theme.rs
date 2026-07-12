//! Shared chrome: border style/type, connected-panel border fusion, and
//! the accent color used for the status bar and the selected tree row.
//! Modeled after eilmeldung's (https://github.com/christo-auer/eilmeldung)
//! rounded, connected-border look, so every bordered widget in illium picks
//! it up from one place instead of scattering `Color::`/`BorderType::`
//! literals across each view module.

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType};

/// Border corners/edges use rounded glyphs everywhere.
const BORDER_TYPE: BorderType = BorderType::Rounded;

/// Overlapping borders between adjacent panels (see [`overlap_right_edge`])
/// fuse into a single joint (e.g. a corner meeting a straight edge becomes
/// a `┬`/`┴`/`┼`) instead of drawing two borders on top of each other.
const BORDER_MERGE: MergeStrategy = MergeStrategy::Fuzzy;

const BORDER_FOCUSED: Color = Color::Cyan;
const BORDER_UNFOCUSED: Color = Color::DarkGray;

/// Lavender accent used for the status bar and the current tree selection,
/// with a near-black foreground for contrast on top of it.
pub const ACCENT_BG: Color = Color::Rgb(0x9d, 0x7c, 0xd8);
pub const ACCENT_FG: Color = Color::Rgb(0x1a, 0x1b, 0x26);

/// Border color/weight for a panel, brighter and bold when `focused`.
pub fn border_style(focused: bool) -> Style {
    let style = Style::new().fg(if focused {
        BORDER_FOCUSED
    } else {
        BORDER_UNFOCUSED
    });
    if focused {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

/// A rounded, mergeable, focus-aware bordered block with no title set yet
/// -- callers chain `.title(...)` for panels that show one.
pub fn block(focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BORDER_TYPE)
        .merge_borders(BORDER_MERGE)
        .border_style(border_style(focused))
}

/// Inert top-border icon cluster (hamburger, big dot, small dot) -- purely
/// decorative, matching every panel's top-left corner in eilmeldung, which
/// shows the same trio (there, they toggle menu/unread/marked filters;
/// illium has no such per-panel filters yet, so these do nothing).
const CHROME_ICONS: &str = "≡ ● ·";

/// Keep the controls visually separate from the rounded panel corner. The
/// border occupies the first cell and these two cells form deliberate inset,
/// rather than relying on the title renderer's one-cell border exclusion.
const CHROME_LEFT_INSET: &str = "  ";

/// Builds a panel's title: the icon cluster alone, or the icon cluster
/// followed by `label` when the panel also has one. Every title uses the
/// same explicit inset so the chrome remains aligned across both panels.
pub fn chrome_title(label: &str) -> Line<'static> {
    if label.is_empty() {
        Line::from(format!("{CHROME_LEFT_INSET}{CHROME_ICONS}"))
    } else {
        Line::from(format!("{CHROME_LEFT_INSET}{CHROME_ICONS} {label}"))
    }
}

/// Nerd Font powerline round-cap glyphs used to give the status bar rounded
/// ends: each is rendered with `fg` = the bar's own accent color and no
/// explicit `bg` (so it blends into whatever sits outside the bar), the
/// same left-half-circle/right-half-circle trick powerline-style prompts
/// and eilmeldung's own status bar icons use.
pub const STATUSBAR_CAP_LEFT: &str = "\u{e0b6}";
pub const STATUSBAR_CAP_RIGHT: &str = "\u{e0b4}";

/// Style for the two status bar end caps: accent-colored glyph, no
/// background of its own, so it reads as a rounded edge rather than a
/// solid block.
pub fn statusbar_cap_style() -> Style {
    Style::new().fg(ACCENT_BG)
}

/// Background fill for the bottom status bar: solid accent, not reversed
/// video, so per-row semantic text colors (e.g. an agent's status color)
/// keep reading correctly wherever else they're reused.
pub fn statusbar_style() -> Style {
    Style::new().fg(ACCENT_FG).bg(ACCENT_BG)
}

/// Background tint for the current tree selection. A plain `bg` fill
/// (rather than `Modifier::REVERSED`) so a row's own status color -- the
/// spinner's yellow, the waiting-approval cyan dot, etc. -- stays legible
/// on top of it instead of being flipped into the background channel.
pub fn selected_style() -> Style {
    Style::new().bg(ACCENT_BG).fg(ACCENT_FG)
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    use super::*;

    /// The production UI deliberately overlaps the two panel borders by one
    /// column. Render that exact arrangement so header spacing and the far
    /// right rounded corner cannot regress independently of live PTY output.
    #[test]
    fn chrome_titles_are_inset_and_preserve_the_right_panel_corner() {
        let backend = TestBackend::new(120, 4);
        let mut terminal = Terminal::new(backend).expect("test backend should initialise");
        let tree_area = Rect::new(0, 0, 33, 4);
        let pane_area = Rect::new(32, 0, 88, 4);
        let long_pane_label = "Codex PID 12345 · Session 01234567-89ab-cdef-0123-456789abcdef";

        terminal
            .draw(|frame| {
                frame.render_widget(block(false).title(chrome_title(long_pane_label)), pane_area);
                frame.render_widget(block(true).title(chrome_title("Illium")), tree_area);
            })
            .expect("header frame should render");

        let buffer = terminal.backend().buffer();

        // The first icon starts two cells after the title area's left edge,
        // which itself begins immediately after the rounded border corner.
        assert_eq!(buffer[(3, 0)].symbol(), "≡");
        assert_eq!(buffer[(35, 0)].symbol(), "≡");
        assert_eq!(buffer[(9, 0)].symbol(), "I");
        assert_eq!(chrome_title("Illium").width(), 14);
        assert_eq!(chrome_title("Illium").to_string(), "  ≡ ● · Illium");

        // A long selected-pane title must be clipped inside the title area,
        // never overwrite the panel's closing rounded corner.
        assert_eq!(buffer[(119, 0)].symbol(), "╮");
    }
}
