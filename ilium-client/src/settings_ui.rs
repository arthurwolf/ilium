//! Renders `crate::app::Mode::Settings`: the full-screen settings view.
//!
//! Design brief (the user's own words -- see `crate::app::SettingsState`'s
//! doc comment for the full quote, kept there since `App` owns the state
//! this module only renders): the **entire** screen is replaced, aerated
//! rather than boxy, tab list on the left / settings panel on the right
//! with *no separating border* between them (unlike every bordered panel
//! in `crate::theme::block`), usable at any terminal size via a scrollbar
//! plus mouse-wheel scrolling, and every control is a live, self-applying
//! toggle/stepper -- never a buffered form with a Save button.
//!
//! Adding a fourth Appearance setting? Add a variant to
//! `crate::app::AppearanceRow` and its `ALL` array, then extend
//! [`appearance_row_label`]/[`appearance_row_description`]/
//! [`appearance_row_value`] and `App::settings_adjust_row` -- row count,
//! scroll bounds, and mouse hit-testing all fall out of `AppearanceRow::ALL`
//! automatically, nothing else here needs to change. Adding a third *tab*
//! is a bigger change: extend `crate::app::SettingsTab::ALL` and add a
//! render/hit-test branch here for it.
//!
//! Rendering and mouse hit-testing both read the same geometry
//! ([`compute_layout`], [`appearance_content_hit`], [`tab_at`]) so the two
//! can never drift apart -- the same pattern `crate::modal`'s create-group
//! dialog and `crate::tree_ui`'s row controls already use.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::app::{App, AppearanceRow, SettingsState, SettingsTab};
use crate::config::UiSettings;
use crate::theme::{self, ColorScheme};

/// Fixed header height: a title line (with the close button right-aligned
/// on it) plus a dim "Esc or q to close" hint line before the tab list /
/// content body begins.
const HEADER_HEIGHT: u16 = 2;
/// Width of the left tab list column.
const TAB_LIST_WIDTH: u16 = 26;
/// Blank columns between the tab list and the content panel -- this, not a
/// border character, is what separates them (see the module doc comment).
const GAP_WIDTH: u16 = 3;

/// One blank line above the first tab, then one line per tab plus one blank
/// spacer line after it -- see [`tab_at`], which must reproduce this exact
/// arithmetic to hit-test a click back to a tab.
const TAB_LIST_TOP_PADDING: u16 = 1;
const TAB_ROW_HEIGHT: u16 = 2;

/// One blank line above the first row, then one label/value line + one dim
/// description line + one blank spacer per row -- see
/// [`appearance_content_hit`], which must reproduce this exact arithmetic.
const APPEARANCE_TOP_PADDING: u16 = 1;
const APPEARANCE_ROW_HEIGHT: u16 = 3;

/// Column where a row's value control (`‹ value ›`) begins -- two leading
/// spaces plus the padded label column. Shared by rendering and
/// [`appearance_content_hit`].
const LABEL_COLUMN_WIDTH: u16 = 40;
const ROW_LEFT_INSET: u16 = 2;
/// Width of the `‹ ` hot zone that decrements/cycles-back a value; a click
/// anywhere else in the control increments/cycles-forward.
const DECREMENT_ZONE_WIDTH: u16 = 2;

/// The screen regions [`render`] draws into and mouse hit-testing reads
/// back out -- computed fresh from `area` every frame/event rather than
/// cached, since it's cheap pure arithmetic (mirrors `crate::layout::UiLayout`
/// in spirit, just for this one overlay instead of the whole screen).
pub struct SettingsLayout {
    pub header_area: Rect,
    pub tab_list_area: Rect,
    pub content_area: Rect,
}

/// Splits `area` into the header/tab-list/content regions. No vertical
/// border between the tab list and content -- [`GAP_WIDTH`] blank columns
/// instead, per the module doc comment's "aerated, not boxy" brief.
pub fn compute_layout(area: Rect) -> SettingsLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(HEADER_HEIGHT), Constraint::Min(1)])
        .split(area);
    let header_area = rows[0];
    let body_area = rows[1];

    let tab_list_width = TAB_LIST_WIDTH.min(body_area.width);
    let gap_width = GAP_WIDTH.min(body_area.width.saturating_sub(tab_list_width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(tab_list_width),
            Constraint::Length(gap_width),
            Constraint::Min(1),
        ])
        .split(body_area);

    SettingsLayout {
        header_area,
        tab_list_area: columns[0],
        content_area: columns[2],
    }
}

/// Label for the clickable close control in the top-right corner of the
/// header -- see [`close_button_hit`] for its matching hit-test rect. Kept
/// short so it reads as a button, not a sentence.
const CLOSE_LABEL: &str = "\u{2715} Close";

/// Draws the full-screen settings view. Callers must skip drawing anything
/// else this frame (see `crate::ui::draw`'s `Mode::Settings` branch) -- this
/// replaces the entire screen, it isn't a popup layered on top. Two ways to
/// leave are always on screen so the view never feels like a trap: the
/// `CLOSE_LABEL` button at the top right (mouse) and the hint line under the
/// title (keyboard) -- both call back to `Mode::Normal`, see
/// `crate::keys::handle_settings_event`/`crate::mouse::handle_settings_mouse`.
pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    frame.render_widget(Clear, area);
    let layout = compute_layout(area);

    render_header(frame, layout.header_area);
    render_tab_list(frame, layout.tab_list_area, state.tab);
    match state.tab {
        SettingsTab::Appearance => {
            let lines = appearance_lines(&app.ui_settings, state.selected_row);
            render_scrollable(frame, layout.content_area, lines, state.scroll);
        }
        SettingsTab::About => {
            render_scrollable(frame, layout.content_area, about_lines(), state.scroll);
        }
    }
}

/// Renders the title line (with the [`CLOSE_LABEL`] button right-aligned on
/// it) and a dim keyboard-hint line beneath it -- see [`close_button_hit`]
/// for the button's hit-test rect, which must track this layout.
fn render_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(Span::styled(
        "\u{2699} Settings",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let close = Line::from(Span::styled(
        CLOSE_LABEL,
        theme::selected_style().add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Right);

    frame.render_widget(Paragraph::new(title), area);
    frame.render_widget(Paragraph::new(close), area);

    if area.height > 1 {
        let hint_area = Rect::new(area.x, area.y + 1, area.width, 1);
        let hint = Line::from(Span::styled(
            "Esc or q to close",
            Style::new().add_modifier(Modifier::DIM),
        ));
        frame.render_widget(Paragraph::new(hint), hint_area);
    }
}

/// The header row's `CLOSE_LABEL` button rect, right-aligned exactly as
/// [`render_header`] draws it -- a click anywhere in this rect closes
/// settings, same as `Esc`/`q`. See `crate::mouse::handle_settings_mouse`.
pub fn close_button_hit(header_area: Rect, position: Position) -> bool {
    let label_width = CLOSE_LABEL.chars().count() as u16;
    if header_area.height == 0 || label_width > header_area.width {
        return false;
    }
    let button_area = Rect::new(
        header_area.right() - label_width,
        header_area.y,
        label_width,
        1,
    );
    button_area.contains(position)
}

/// One blank top-padding line, then each tab's label with one blank spacer
/// line after it -- see [`tab_at`] for the matching hit-test arithmetic.
fn render_tab_list(frame: &mut Frame, area: Rect, active: SettingsTab) {
    let mut lines = Vec::with_capacity(1 + SettingsTab::ALL.len() * usize::from(TAB_ROW_HEIGHT));
    lines.push(Line::from(""));
    for tab in SettingsTab::ALL {
        let style = if tab == active {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", tab.label()),
            style,
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The tab a click at `position` lands on, if any -- reproduces
/// [`render_tab_list`]'s exact line arithmetic rather than re-deriving it.
pub fn tab_at(tab_list_area: Rect, position: Position) -> Option<SettingsTab> {
    if !tab_list_area.contains(position) {
        return None;
    }
    let row = position.y - tab_list_area.y;
    if row < TAB_LIST_TOP_PADDING {
        return None;
    }
    let row = row - TAB_LIST_TOP_PADDING;
    if !row.is_multiple_of(TAB_ROW_HEIGHT) {
        return None;
    }
    let index = usize::from(row / TAB_ROW_HEIGHT);
    SettingsTab::ALL.get(index).copied()
}

/// Renders `lines` scrolled by `scroll` rows, adding a vertical scrollbar
/// only once the content is actually taller than `area` -- matches
/// `crate::ui`'s own `draw_terminal_scrollbar`/`draw_rendered_scrollbar`.
fn render_scrollable(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: u16) {
    let total_lines = lines.len() as u16;
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);

    if total_lines > area.height {
        let mut scrollbar_state =
            ScrollbarState::new(usize::from(total_lines)).position(usize::from(scroll));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .style(theme::border_style(false));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// The largest valid `scroll` value for `tab` at `content_area`'s current
/// height -- callers (`crate::keys`/`crate::mouse`) clamp scroll-wheel and
/// keyboard scrolling to this so the view can never scroll past its own end.
pub fn max_scroll(
    tab: SettingsTab,
    ui: &UiSettings,
    selected_row: usize,
    content_area: Rect,
) -> u16 {
    let total_lines = match tab {
        SettingsTab::Appearance => appearance_lines(ui, selected_row).len() as u16,
        SettingsTab::About => about_lines().len() as u16,
    };
    total_lines.saturating_sub(content_area.height)
}

fn appearance_row_label(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => "Auto-resize tree panel on focus",
        AppearanceRow::TreeWidth => "Tree panel width",
        AppearanceRow::ColorScheme => "Color theme",
    }
}

fn appearance_row_description(row: AppearanceRow) -> &'static str {
    match row {
        AppearanceRow::AutoResizeTree => {
            "Widen the tree panel while it has mouse or keyboard focus, then ease back."
        }
        AppearanceRow::TreeWidth => "Collapsed width of the tree panel, in terminal columns.",
        AppearanceRow::ColorScheme => "ilium's built-in color presets.",
    }
}

fn appearance_row_value(row: AppearanceRow, ui: &UiSettings) -> String {
    match row {
        AppearanceRow::AutoResizeTree => {
            if ui.auto_resize_tree_on_focus {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        AppearanceRow::TreeWidth => ui.tree_width.to_string(),
        AppearanceRow::ColorScheme => match ui.color_scheme {
            ColorScheme::Dark => "Dark".to_string(),
            ColorScheme::Light => "Light".to_string(),
        },
    }
}

/// One label/value line per `AppearanceRow`, each followed by a dim
/// description line and a blank spacer -- see [`appearance_content_hit`]
/// for the matching hit-test arithmetic, and the module doc comment for why
/// both read `AppearanceRow::ALL` instead of hardcoding a row count.
fn appearance_lines(ui: &UiSettings, selected_row: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(
        usize::from(APPEARANCE_TOP_PADDING)
            + AppearanceRow::ALL.len() * usize::from(APPEARANCE_ROW_HEIGHT),
    );
    lines.push(Line::from(""));
    for (index, row) in AppearanceRow::ALL.into_iter().enumerate() {
        let selected = index == selected_row;
        lines.push(appearance_row_line(row, ui, selected));
        lines.push(Line::from(Span::styled(
            format!("    {}", appearance_row_description(row)),
            Style::new().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(""));
    }
    lines
}

fn appearance_row_line(row: AppearanceRow, ui: &UiSettings, selected: bool) -> Line<'static> {
    let label = appearance_row_label(row);
    let value = appearance_row_value(row, ui);
    let control = format!("\u{2039} {value} \u{203a}");

    let padding = usize::from(LABEL_COLUMN_WIDTH).saturating_sub(label.chars().count());
    let label_style = if selected {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    let control_style = if selected {
        theme::selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme::accent_bg())
    };

    Line::from(vec![
        Span::raw(" ".repeat(usize::from(ROW_LEFT_INSET))),
        Span::styled(label, label_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(control, control_style),
    ])
}

/// The `(row, direction)` a click at `position` should adjust, if any.
/// `direction` is `-1` for the `‹` hot zone, `+1` for anywhere else in the
/// control -- see `App::settings_adjust_row`. Reproduces
/// [`appearance_lines`]'s exact line arithmetic (top padding, three lines
/// per row) and [`appearance_row_line`]'s exact column arithmetic (left
/// inset, padded label column) rather than re-deriving either.
pub fn appearance_content_hit(
    content_area: Rect,
    scroll: u16,
    position: Position,
) -> Option<(AppearanceRow, i32)> {
    if !content_area.contains(position) {
        return None;
    }

    let line_index = i32::from(position.y) - i32::from(content_area.y) + i32::from(scroll);
    let line_index = line_index - i32::from(APPEARANCE_TOP_PADDING);
    if line_index < 0 || line_index % i32::from(APPEARANCE_ROW_HEIGHT) != 0 {
        return None;
    }
    let row_index = usize::try_from(line_index / i32::from(APPEARANCE_ROW_HEIGHT)).ok()?;
    let row = AppearanceRow::ALL.get(row_index).copied()?;

    let control_start_x = content_area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
    if position.x < control_start_x {
        return None;
    }
    let direction = if position.x < control_start_x + DECREMENT_ZONE_WIDTH {
        -1
    } else {
        1
    };
    Some((row, direction))
}

/// Static "About" content: version plus a short description. No settings
/// live here, per the design brief -- see the module doc comment.
fn about_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "ilium",
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        Line::from(""),
        Line::from("A terminal multiplexer with a tree-structured pane list and AI"),
        Line::from("coding agent awareness (Claude Code, Codex CLI, and similar tools)."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_layout_leaves_no_border_gap_between_tabs_and_content() {
        let layout = compute_layout(Rect::new(0, 0, 100, 40));
        assert!(layout.content_area.x > layout.tab_list_area.right());
        assert_eq!(
            layout.content_area.x - layout.tab_list_area.right(),
            GAP_WIDTH
        );
    }

    #[test]
    fn tab_at_maps_each_row_to_its_tab() {
        let area = Rect::new(0, 0, 30, 20);
        assert_eq!(
            tab_at(area, Position::new(2, 1)),
            Some(SettingsTab::Appearance)
        );
        assert_eq!(tab_at(area, Position::new(2, 3)), Some(SettingsTab::About));
        // Row 0 is the top-padding blank line -- no tab there.
        assert_eq!(tab_at(area, Position::new(2, 0)), None);
        // Row 2 is the blank spacer after the first tab.
        assert_eq!(tab_at(area, Position::new(2, 2)), None);
    }

    #[test]
    fn appearance_content_hit_finds_each_row_and_the_correct_zone() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;

        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 1)),
            Some((AppearanceRow::AutoResizeTree, -1))
        );
        assert_eq!(
            appearance_content_hit(
                area,
                0,
                Position::new(control_start_x + DECREMENT_ZONE_WIDTH, 1)
            ),
            Some((AppearanceRow::AutoResizeTree, 1))
        );
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 4)),
            Some((AppearanceRow::TreeWidth, -1))
        );
        // A click on the label (left of the control) is a no-op.
        assert_eq!(appearance_content_hit(area, 0, Position::new(2, 1)), None);
        // A click on the description line between rows is a no-op.
        assert_eq!(
            appearance_content_hit(area, 0, Position::new(control_start_x, 2)),
            None
        );
    }

    #[test]
    fn appearance_content_hit_accounts_for_scroll_offset() {
        let area = Rect::new(0, 0, 60, 20);
        let control_start_x = area.x + ROW_LEFT_INSET + LABEL_COLUMN_WIDTH;
        // Scrolled down by 3 lines, row 1 on screen is TreeWidth's line
        // (originally at line 4).
        assert_eq!(
            appearance_content_hit(area, 3, Position::new(control_start_x, 1)),
            Some((AppearanceRow::TreeWidth, -1))
        );
    }

    #[test]
    fn max_scroll_is_zero_when_content_already_fits() {
        let ui = UiSettings::default();
        let area = Rect::new(0, 0, 60, 40);
        assert_eq!(max_scroll(SettingsTab::Appearance, &ui, 0, area), 0);
    }

    #[test]
    fn max_scroll_is_positive_when_content_overflows_a_short_terminal() {
        let ui = UiSettings::default();
        let area = Rect::new(0, 0, 60, 3);
        assert!(max_scroll(SettingsTab::Appearance, &ui, 0, area) > 0);
    }

    #[test]
    fn close_button_hit_covers_the_right_aligned_label_only() {
        let header_area = Rect::new(0, 0, 40, 2);
        let label_width = CLOSE_LABEL.chars().count() as u16;

        // Inside the label, top row.
        assert!(close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 0)
        ));
        // Just left of the label -- still the title, not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - label_width - 1, 0)
        ));
        // Same column, but the hint line below the label -- not a hit.
        assert!(!close_button_hit(
            header_area,
            Position::new(header_area.right() - 1, 1)
        ));
    }

    #[test]
    fn close_button_hit_is_false_when_header_too_narrow_for_the_label() {
        let header_area = Rect::new(0, 0, 3, 2);
        assert!(!close_button_hit(header_area, Position::new(1, 0)));
    }
}
