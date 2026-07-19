//! Small, centered popup widgets shared by every blocking prompt: a
//! single-line text input (rename, run-command) and a Yes/No confirmation
//! (close). Both draw over a `Clear`+`Block` exactly like the existing
//! context-menu/help/explorer overlays in `ui.rs`/`help.rs`, so every modal
//! in ilium reads as one visual language instead of three.

use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::text_prompt::TextPromptState;
use crate::theme;

/// Fixed width of the "New group" destination picker (see
/// `App::open_create_group_dialog`) -- wide enough for a few levels of
/// indentation plus a realistic group name without wrapping.
const CREATE_GROUP_WIDTH: u16 = 54;

/// At most this many destinations are shown at once; a session with more
/// nested groups than this scrolls the list (see `create_group_visible_window`)
/// rather than growing the popup past a reasonable screen fraction.
pub const CREATE_GROUP_MAX_VISIBLE: usize = 7;
pub const CREATE_SPLIT_MEMBER_MAX_VISIBLE: usize = 9;

pub fn create_split_orientation_dialog_area(screen_area: Rect) -> Rect {
    centered_fixed_rect(62, 9, screen_area)
}

pub fn create_split_members_dialog_area(screen_area: Rect, choice_count: usize) -> Rect {
    let visible_count = choice_count.clamp(1, CREATE_SPLIT_MEMBER_MAX_VISIBLE) as u16;
    centered_fixed_rect(72, visible_count + 6, screen_area)
}

pub fn create_split_member_visible_window(
    selected_index: usize,
    choice_count: usize,
) -> (usize, usize) {
    let start = selected_index
        .saturating_sub(CREATE_SPLIT_MEMBER_MAX_VISIBLE / 2)
        .min(choice_count.saturating_sub(CREATE_SPLIT_MEMBER_MAX_VISIBLE));
    (
        start,
        (start + CREATE_SPLIT_MEMBER_MAX_VISIBLE).min(choice_count),
    )
}

pub fn create_split_member_row_at(
    screen_area: Rect,
    selected_index: usize,
    choice_count: usize,
    position: Position,
) -> Option<usize> {
    let popup = create_split_members_dialog_area(screen_area, choice_count);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let first_choice_row = inner.y.saturating_add(2);
    if position.x < inner.x || position.x >= inner.right() || position.y < first_choice_row {
        return None;
    }
    let (start, end) = create_split_member_visible_window(selected_index, choice_count);
    let index = start + usize::from(position.y.saturating_sub(first_choice_row));
    (index < end).then_some(index)
}

/// Interior row layout of the create-group dialog, computed purely from its
/// outer `area` -- shared by rendering (`ui::draw_create_group`) and mouse
/// hit-testing (`App::handle_create_group_mouse`) so neither can drift out
/// of sync with the other.
pub struct CreateGroupLayout {
    pub name_row: Rect,
    pub label_row: Rect,
    pub list_area: Rect,
    pub hint_row: Rect,
}

/// The outer popup rect, sized to fit every visible destination row (capped
/// at `CREATE_GROUP_MAX_VISIBLE`) plus the fixed name/label/hint chrome.
pub fn create_group_dialog_area(screen_area: Rect, destination_count: usize) -> Rect {
    let visible = destination_count.clamp(1, CREATE_GROUP_MAX_VISIBLE);
    // Border (2) + name row (1) + spacer (1) + "Create under:" label (1) +
    // one row per visible destination + hint row (1).
    let height = 2 + 1 + 1 + 1 + visible as u16 + 1;
    centered_fixed_rect(CREATE_GROUP_WIDTH, height, screen_area)
}

/// Splits a create-group dialog's outer `area` into its fixed rows. Assumes
/// a one-cell border on every edge, matching `theme::block`.
pub fn create_group_layout(area: Rect) -> CreateGroupLayout {
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // name field
            Constraint::Length(1), // spacer
            Constraint::Length(1), // "Create under:" label
            Constraint::Min(0),    // destination list
            Constraint::Length(1), // hint
        ])
        .split(inner);
    CreateGroupLayout {
        name_row: rows[0],
        label_row: rows[2],
        list_area: rows[3],
        hint_row: rows[4],
    }
}

/// The `[start, end)` slice of `total` destinations to actually render,
/// keeping `selected_index` in view. A pure function of the three inputs so
/// scroll position never needs its own persisted, driftable state.
pub fn create_group_visible_window(
    selected_index: usize,
    total: usize,
    capacity: usize,
) -> (usize, usize) {
    if total <= capacity {
        return (0, total);
    }
    let half = capacity / 2;
    let start = selected_index.saturating_sub(half).min(total - capacity);
    (start, start + capacity)
}

/// Maps a screen position to the destination index it lands on, or `None`
/// if it's outside the visible list rows. `window` is the `(start, end)`
/// pair from `create_group_visible_window` for the same `selected_index`/
/// `total` the caller rendered.
pub fn create_group_row_at(
    layout: &CreateGroupLayout,
    window: (usize, usize),
    position: Position,
) -> Option<usize> {
    if !layout.list_area.contains(position) {
        return None;
    }
    let row_in_window = (position.y - layout.list_area.y) as usize;
    let (start, end) = window;
    let index = start + row_in_window;
    (index < end).then_some(index)
}

/// A `width` x `height` rect centered within `area`, clamped so it never
/// exceeds the available screen (a narrow terminal shrinks the popup rather
/// than panicking or drawing off-screen).
pub fn centered_fixed_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1])[1]
}

/// Fixed size of the text-prompt popup: wide enough for a typical pane/group
/// name or shell command, three rows tall (border, input line, border) plus
/// one for the hint.
const PROMPT_WIDTH: u16 = 50;
const PROMPT_HEIGHT: u16 = 4;

/// Draws the rename/run-command popup and places the real terminal cursor
/// inside it -- a blinking, native cursor reads as a proper text input
/// rather than the plain "RENAME: foo" the bottom status bar used to show,
/// and it's exact (no hand-drawn cursor glyph to keep in sync with cursor
/// movement).
pub fn render_text_prompt(
    frame: &mut Frame,
    screen_area: Rect,
    title: &str,
    state: &TextPromptState,
) {
    let area = centered_fixed_rect(PROMPT_WIDTH, PROMPT_HEIGHT, screen_area);
    frame.render_widget(Clear, area);

    let block = theme::block(true).title(theme::chrome_title(title));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Paragraph::new(state.buf.as_str()), rows[0]);
    frame.render_widget(
        Paragraph::new("Enter to confirm · Esc to cancel")
            .style(Style::new().add_modifier(Modifier::DIM)),
        rows[1],
    );

    // ilium's prompts only ever hold pane/group names and shell command
    // lines -- ordinary Latin text -- so one `char` == one terminal cell.
    // A wide-character-aware cursor would need `unicode-width`, which
    // nothing else in this crate depends on yet.
    // `saturating_add` before the clamp -- `state.cursor` grows with every
    // typed/pasted character and is otherwise unbounded, so `rows[0].x +
    // cursor` could overflow `u16` (panic in debug, wrap to a bogus column
    // in release) before the `.min()` below ever got a chance to clamp it.
    let cursor_x = rows[0]
        .x
        .saturating_add(u16::try_from(state.cursor).unwrap_or(u16::MAX));
    // `Rect::right()` is exclusive (the first column *outside* the rect), so
    // clamping to it directly would let the cursor land on the block's right
    // border instead of the last real cell of the input row.
    frame.set_cursor_position(Position::new(
        cursor_x.min(rows[0].right().saturating_sub(1)),
        rows[0].y,
    ));
}

/// Credential variant of [`render_text_prompt`]. The editable buffer stays
/// real so normal cursor/edit semantics work, while no API-key character is
/// ever painted into the terminal or its scrollback.
pub fn render_masked_text_prompt(
    frame: &mut Frame,
    screen_area: Rect,
    title: &str,
    state: &TextPromptState,
) {
    let area = centered_fixed_rect(PROMPT_WIDTH, PROMPT_HEIGHT, screen_area);
    frame.render_widget(Clear, area);
    let block = theme::block(true).title(theme::chrome_title(title));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new("•".repeat(state.buf.chars().count())),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("Enter to replace · Esc to keep the existing key")
            .style(Style::new().add_modifier(Modifier::DIM)),
        rows[1],
    );
    let cursor_x = rows[0]
        .x
        .saturating_add(u16::try_from(state.cursor).unwrap_or(u16::MAX));
    frame.set_cursor_position(Position::new(
        cursor_x.min(rows[0].right().saturating_sub(1)),
        rows[0].y,
    ));
}

/// Large multiline text area used by the Voice control prompt editor.
pub fn render_multiline_prompt(
    frame: &mut Frame,
    screen_area: Rect,
    title: &str,
    textarea: &ratatui_textarea::TextArea<'static>,
) {
    let width = screen_area.width.saturating_sub(8).clamp(40, 100);
    let height = screen_area.height.saturating_sub(6).clamp(8, 28);
    let area = centered_fixed_rect(width, height, screen_area);
    frame.render_widget(Clear, area);
    let block = theme::block(true).title(theme::chrome_title(title));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(textarea, rows[0]);
    frame.render_widget(
        Paragraph::new("Ctrl+S to apply · Esc to cancel · Enter inserts a line")
            .style(Style::new().add_modifier(Modifier::DIM)),
        rows[1],
    );
}

/// Size of the confirmation popup: enough for a two-line message plus the
/// Yes/No hint.
const CONFIRM_WIDTH: u16 = 54;
const CONFIRM_HEIGHT: u16 = 5;

/// Draws a Yes/No confirmation popup, styled with a warning accent so a
/// destructive action (closing a group with children, or an editor with
/// unsaved changes) reads as distinct from the neutral rename/run-command
/// prompts.
pub fn render_confirm(frame: &mut Frame, screen_area: Rect, title: &str, message: &str) {
    let area = centered_fixed_rect(CONFIRM_WIDTH, CONFIRM_HEIGHT, screen_area);
    frame.render_widget(Clear, area);

    let warning_style = Style::new().fg(ratatui::style::Color::Yellow);
    let block = theme::block(true)
        .title(theme::chrome_title(title))
        .border_style(warning_style.add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The message row gets 2 lines (see `CONFIRM_HEIGHT`'s doc comment) with
    // wrapping enabled -- a plain `Length(1)` + no-wrap `Paragraph` silently
    // clipped every real confirmation (e.g. `"\"backend-services\" contains
    // 5 item(s). Close it and everything inside?"` is well past the inner
    // width), so the message needs both the extra row and `Wrap` to actually
    // use it.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), rows[0]);
    let hint = Line::from(vec![
        Span::styled(
            "y",
            warning_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::raw("es  /  "),
        Span::styled(
            "n",
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::raw("o / Esc"),
    ]);
    frame.render_widget(Paragraph::new(hint), rows[1]);
}
