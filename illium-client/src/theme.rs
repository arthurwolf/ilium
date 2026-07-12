//! Shared chrome: border style/type, connected-panel border fusion, and
//! the accent color used for the status bar and the selected tree row.
//! Modeled after eilmeldung's (https://github.com/christo-auer/eilmeldung)
//! rounded, connected-border look, so every bordered widget in illium picks
//! it up from one place instead of scattering `Color::`/`BorderType::`
//! literals across each view module.
//!
//! The actual colors ([`Theme`]) are configurable via `config.toml`'s
//! `[theme]` table (`crate::config::load`) -- covers the four colors below
//! (accent background/foreground, focused/unfocused border) since those
//! are illium's most visually prominent choices; every other visual
//! (border type/merge strategy, the chrome icon glyphs, status bar caps)
//! stays a hardcoded constant, not every `ratatui::style::Style` in the
//! app is themeable.

use std::sync::OnceLock;

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

/// The illium color palette every bordered/accented widget reads from.
/// `Theme::default()` reproduces the exact hardcoded values this crate
/// shipped with before config support existed -- a user with no
/// `config.toml` `[theme]` table must see byte-identical rendering to
/// before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Lavender accent used for the status bar and the current tree
    /// selection.
    pub accent_bg: Color,
    /// Near-black foreground for contrast on top of `accent_bg`.
    pub accent_fg: Color,
    /// Border color for a panel that currently has focus.
    pub border_focused: Color,
    /// Border color for a panel that does not.
    pub border_unfocused: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent_bg: Color::Rgb(0x9d, 0x7c, 0xd8),
            accent_fg: Color::Rgb(0x1a, 0x1b, 0x26),
            border_focused: Color::Cyan,
            border_unfocused: Color::DarkGray,
        }
    }
}

/// The effective theme for the process's lifetime, installed once at
/// startup (`crate::run`, after `crate::config::load`) and never mutated
/// again.
///
/// This is a legitimate use of a global, not a smell: illium-client is a
/// single-process TUI with exactly one render loop, the theme is resolved
/// once before that loop's first frame and never changes afterward (there
/// is no live "reload theme" request), and every render call site
/// (`ui.rs`/`tree_ui.rs`/`modal.rs`/`help.rs`/`explorer_overlay.rs`) already
/// calls straight into this module's free functions with no `&App`/state
/// threaded through them -- adding a `&Theme` parameter to every one of
/// those call sites across every render module would be a large,
/// mechanical, purely-plumbing change for no behavioral benefit over one
/// `OnceLock` read at the bottom of each function.
static THEME: OnceLock<Theme> = OnceLock::new();

/// Installs the effective theme for the rest of the process's lifetime --
/// called once at client startup. A second call is a no-op (there is no
/// "reload config" request yet, so nothing should ever attempt one).
pub fn init(theme: Theme) {
    let _ = THEME.set(theme);
}

/// The theme currently in effect. Falls back to [`Theme::default`] when
/// [`init`] hasn't run yet (every unit test in this crate, and any render
/// call that could theoretically happen before startup finishes) rather
/// than panicking -- a themeable value defaulting to "the value it always
/// had before this feature existed" is never a bug worth crashing over.
fn current() -> Theme {
    THEME.get().copied().unwrap_or_default()
}

/// The effective accent background color -- exposed directly (not just via
/// `statusbar_style`/`selected_style`) for the handful of call sites that
/// need to compose it with another color themselves (e.g.
/// `tree_ui.rs::draw_toolbar`'s hovered-button style, which layers it over
/// a per-action accent).
pub fn accent_bg() -> Color {
    current().accent_bg
}

/// The effective accent foreground color -- see [`accent_bg`]'s doc
/// comment for why this is exposed as a plain `Color` getter too.
pub fn accent_fg() -> Color {
    current().accent_fg
}

/// Border color/weight for a panel, brighter and bold when `focused`.
pub fn border_style(focused: bool) -> Style {
    let theme = current();
    let style = Style::new().fg(if focused {
        theme.border_focused
    } else {
        theme.border_unfocused
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
    Style::new().fg(current().accent_bg)
}

/// Background fill for the bottom status bar: solid accent, not reversed
/// video, so per-row semantic text colors (e.g. an agent's status color)
/// keep reading correctly wherever else they're reused.
pub fn statusbar_style() -> Style {
    let theme = current();
    Style::new().fg(theme.accent_fg).bg(theme.accent_bg)
}

/// Background tint for the current tree selection. A plain `bg` fill
/// (rather than `Modifier::REVERSED`) so a row's own bold weight -- e.g. a
/// waiting-approval row's emphasis -- stays legible on top of it instead of
/// being flipped into the background channel.
pub fn selected_style() -> Style {
    let theme = current();
    Style::new().bg(theme.accent_bg).fg(theme.accent_fg)
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

    /// With no `init` call (the state every test in this binary runs
    /// under, since `init` is only ever called once, for real, from
    /// `crate::run`), every color getter must read back the exact
    /// pre-config-support hardcoded values -- a no-config-file user's
    /// rendering must stay byte-identical.
    #[test]
    fn colors_default_to_the_original_hardcoded_values_when_uninitialised() {
        assert_eq!(accent_bg(), Color::Rgb(0x9d, 0x7c, 0xd8));
        assert_eq!(accent_fg(), Color::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(border_style(true).fg, Some(Color::Cyan));
        assert_eq!(border_style(false).fg, Some(Color::DarkGray));
    }
}
