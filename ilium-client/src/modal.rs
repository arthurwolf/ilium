//! Small, centered popup widgets shared by every blocking prompt: a
//! single-line text input (rename, run-command) and a Yes/No confirmation
//! (close). Both draw over a `Clear`+`Block` exactly like the existing
//! context-menu/help/explorer overlays in `ui.rs`/`help.rs`, so every modal
//! in ilium reads as one visual language instead of three.

use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
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

/// Semantic result of clicking one of a blocking dialog's action buttons.
/// Keyboard handlers remain authoritative; `crate::mouse` translates this
/// result back into the corresponding Enter/Esc or Y/N key contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    Cancel,
    Confirm,
}

/// Visual meaning of one action button. A danger tone is reserved for the
/// action that actually destroys data or closes a tree item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButtonTone {
    Neutral,
    Primary,
    Danger,
}

/// Text and keyboard affordance rendered inside one action button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogButton<'a> {
    pub key_hint: &'a str,
    pub label: &'a str,
    pub tone: DialogButtonTone,
}

/// Ordered cancel/confirm actions shared by rendering and hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogActions<'a> {
    pub cancel: DialogButton<'a>,
    pub confirm: DialogButton<'a>,
}

impl<'a> DialogActions<'a> {
    /// Standard form actions retain Esc/Enter as the keyboard equivalents.
    pub const fn form(cancel_label: &'a str, confirm_label: &'a str) -> Self {
        Self {
            cancel: DialogButton {
                key_hint: "Esc",
                label: cancel_label,
                tone: DialogButtonTone::Neutral,
            },
            confirm: DialogButton {
                key_hint: "Enter",
                label: confirm_label,
                tone: DialogButtonTone::Primary,
            },
        }
    }

    /// Yes/No actions retain the existing N/Y shortcuts while allowing each
    /// side to communicate its own safety level.
    pub const fn confirmation(
        cancel_label: &'a str,
        cancel_tone: DialogButtonTone,
        confirm_label: &'a str,
        confirm_tone: DialogButtonTone,
    ) -> Self {
        Self {
            cancel: DialogButton {
                key_hint: "N",
                label: cancel_label,
                tone: cancel_tone,
            },
            confirm: DialogButton {
                key_hint: "Y",
                label: confirm_label,
                tone: confirm_tone,
            },
        }
    }
}

/// Exact rectangles occupied by the two visible action buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogActionLayout {
    pub cancel_button: Rect,
    pub confirm_button: Rect,
}

impl DialogActionLayout {
    /// Maps a pointer position to the same semantic action keyboard input
    /// already exposes.
    pub fn action_at(self, position: Position) -> Option<DialogAction> {
        if self.cancel_button.contains(position) {
            return Some(DialogAction::Cancel);
        }
        if self.confirm_button.contains(position) {
            return Some(DialogAction::Confirm);
        }
        None
    }
}

/// Layout shared by every single-line text-entry dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPromptDialogLayout {
    pub popup: Rect,
    pub input_box: Rect,
    pub input_area: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

/// Layout shared by the multiline Voice prompt editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultilinePromptDialogLayout {
    pub popup: Rect,
    pub editor_area: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

/// Layout shared by every Yes/No confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmDialogLayout {
    pub popup: Rect,
    pub message_area: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

/// Layout for the board-creation form, which needs two text fields plus a
/// storage selector and a path picker before its shared action row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateBoardDialogLayout {
    pub popup: Rect,
    pub name_box: Rect,
    pub name_input: Rect,
    pub storage_row: Rect,
    pub path_box: Rect,
    pub path_input: Rect,
    pub browse_button: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

/// Fixed size of a single-line prompt. The extra vertical space gives the
/// input and both mouse targets room to read as real controls.
const PROMPT_WIDTH: u16 = 68;
const PROMPT_HEIGHT: u16 = 10;

/// Computes the authoritative single-line prompt geometry.
pub fn text_prompt_dialog_layout(screen_area: Rect) -> TextPromptDialogLayout {
    let popup = centered_fixed_rect(PROMPT_WIDTH, PROMPT_HEIGHT, screen_area);
    let inner = inset_rect(popup, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    let input_box = rows[1];
    TextPromptDialogLayout {
        popup,
        input_box,
        input_area: inset_rect(input_box, 1),
        actions: dialog_action_layout(rows[3]),
        hint_row: rows[4],
    }
}

/// Size-only entry point for PTY integration tests that intentionally do not
/// depend on ratatui themselves.
pub fn text_prompt_dialog_layout_for_size(width: u16, height: u16) -> TextPromptDialogLayout {
    text_prompt_dialog_layout(Rect::new(0, 0, width, height))
}

/// Computes the responsive multiline prompt geometry.
pub fn multiline_prompt_dialog_layout(screen_area: Rect) -> MultilinePromptDialogLayout {
    let width = screen_area.width.saturating_sub(8).clamp(40, 100);
    let height = screen_area.height.saturating_sub(6).clamp(11, 30);
    let popup = centered_fixed_rect(width, height, screen_area);
    let inner = inset_rect(popup, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    MultilinePromptDialogLayout {
        popup,
        editor_area: rows[0],
        actions: dialog_action_layout(rows[2]),
        hint_row: rows[3],
    }
}

/// Computes spacious board-form geometry shared by rendering and mouse input.
pub fn create_board_dialog_layout(screen_area: Rect) -> CreateBoardDialogLayout {
    let popup = centered_fixed_rect(78, 20, screen_area);
    let inner = Rect::new(
        popup.x.saturating_add(3),
        popup.y.saturating_add(2),
        popup.width.saturating_sub(6),
        popup.height.saturating_sub(4),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    let browse_button = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(18)])
        .split(rows[5])[0];
    CreateBoardDialogLayout {
        popup,
        name_box: rows[0],
        name_input: inset_rect(rows[0], 1),
        storage_row: rows[2],
        path_box: rows[4],
        path_input: inset_rect(rows[4], 1),
        browse_button,
        actions: dialog_action_layout(rows[7]),
        hint_row: rows[8],
    }
}

/// Size-only board-form geometry for the real-binary PTY suite.
pub fn create_board_dialog_layout_for_size(width: u16, height: u16) -> CreateBoardDialogLayout {
    create_board_dialog_layout(Rect::new(0, 0, width, height))
}

/// Insets a rectangle without underflow on very small terminals.
fn inset_rect(area: Rect, margin: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(margin),
        area.y.saturating_add(margin),
        area.width.saturating_sub(margin.saturating_mul(2)),
        area.height.saturating_sub(margin.saturating_mul(2)),
    )
}

/// Gives each button nearly half of the available row, separated by a
/// deliberate two-cell gutter that remains usable on narrow screens.
fn dialog_action_layout(row: Rect) -> DialogActionLayout {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .flex(Flex::Center)
        .split(row);
    DialogActionLayout {
        cancel_button: columns[0],
        confirm_button: columns[2],
    }
}

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
    confirm_label: &str,
) {
    let layout = text_prompt_dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);

    let block = theme::block(true).title(theme::chrome_title(title));
    frame.render_widget(block, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Value")),
        layout.input_box,
    );
    frame.render_widget(Paragraph::new(state.buf.as_str()), layout.input_area);
    render_dialog_actions(
        frame,
        layout.actions,
        DialogActions::form("Cancel", confirm_label),
    );
    frame.render_widget(
        Paragraph::new("Click a button or use the shown keyboard shortcut")
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint_row,
    );

    // ilium's prompts only ever hold pane/group names and shell command
    // lines -- ordinary Latin text -- so one `char` == one terminal cell.
    // A wide-character-aware cursor would need `unicode-width`, which
    // nothing else in this crate depends on yet.
    // `saturating_add` before the clamp -- `state.cursor` grows with every
    // typed/pasted character and is otherwise unbounded, so `rows[0].x +
    // cursor` could overflow `u16` (panic in debug, wrap to a bogus column
    // in release) before the `.min()` below ever got a chance to clamp it.
    let cursor_x = layout
        .input_area
        .x
        .saturating_add(u16::try_from(state.cursor).unwrap_or(u16::MAX));
    // `Rect::right()` is exclusive (the first column *outside* the rect), so
    // clamping to it directly would let the cursor land on the block's right
    // border instead of the last real cell of the input row.
    frame.set_cursor_position(Position::new(
        cursor_x.min(layout.input_area.right().saturating_sub(1)),
        layout.input_area.y,
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
    confirm_label: &str,
) {
    let layout = text_prompt_dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    let block = theme::block(true).title(theme::chrome_title(title));
    frame.render_widget(block, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Protected value")),
        layout.input_box,
    );
    frame.render_widget(
        Paragraph::new("•".repeat(state.buf.chars().count())),
        layout.input_area,
    );
    render_dialog_actions(
        frame,
        layout.actions,
        DialogActions::form("Keep existing", confirm_label),
    );
    frame.render_widget(
        Paragraph::new("The stored value remains hidden")
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint_row,
    );
    let cursor_x = layout
        .input_area
        .x
        .saturating_add(u16::try_from(state.cursor).unwrap_or(u16::MAX));
    frame.set_cursor_position(Position::new(
        cursor_x.min(layout.input_area.right().saturating_sub(1)),
        layout.input_area.y,
    ));
}

/// Large multiline text area used by the Voice control prompt editor.
pub fn render_multiline_prompt(
    frame: &mut Frame,
    screen_area: Rect,
    title: &str,
    textarea: &ratatui_textarea::TextArea<'static>,
    confirm_label: &str,
) {
    let layout = multiline_prompt_dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);
    let block = theme::block(true).title(theme::chrome_title(title));
    frame.render_widget(block, layout.popup);
    frame.render_widget(textarea, layout.editor_area);
    render_dialog_actions(
        frame,
        layout.actions,
        DialogActions::form("Cancel", confirm_label),
    );
    frame.render_widget(
        Paragraph::new("Enter inserts a line · Ctrl+S also applies")
            .style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint_row,
    );
}

/// Size of the confirmation popup: three wrapped message rows, spacious
/// separation, two full-width buttons, and a keyboard reminder.
const CONFIRM_WIDTH: u16 = 68;
const CONFIRM_HEIGHT: u16 = 10;

/// Computes the authoritative confirmation geometry.
pub fn confirm_dialog_layout(screen_area: Rect) -> ConfirmDialogLayout {
    let popup = centered_fixed_rect(CONFIRM_WIDTH, CONFIRM_HEIGHT, screen_area);
    let inner = inset_rect(popup, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    ConfirmDialogLayout {
        popup,
        message_area: rows[1],
        actions: dialog_action_layout(rows[3]),
        hint_row: rows[4],
    }
}

/// Size-only confirmation geometry for the real-binary PTY suite.
pub fn confirm_dialog_layout_for_size(width: u16, height: u16) -> ConfirmDialogLayout {
    confirm_dialog_layout(Rect::new(0, 0, width, height))
}

/// Draws a Yes/No confirmation popup, styled with a warning accent so a
/// destructive action (closing a group with children, or an editor with
/// unsaved changes) reads as distinct from the neutral rename/run-command
/// prompts.
pub fn render_confirm(
    frame: &mut Frame,
    screen_area: Rect,
    title: &str,
    message: &str,
    actions: DialogActions<'_>,
) {
    let layout = confirm_dialog_layout(screen_area);
    frame.render_widget(Clear, layout.popup);

    let warning_style = Style::new().fg(Color::Yellow);
    let block = theme::block(true)
        .title(theme::chrome_title(title))
        .border_style(warning_style.add_modifier(Modifier::BOLD));
    frame.render_widget(block, layout.popup);

    // The message row gets 3 lines (see `CONFIRM_HEIGHT`'s doc comment) with
    // wrapping enabled -- a plain `Length(1)` + no-wrap `Paragraph` silently
    // clipped every real confirmation (e.g. `"\"backend-services\" contains
    // 5 item(s). Close it and everything inside?"` is well past the inner
    // width), so the message needs both the extra row and `Wrap` to actually
    // use it.
    frame.render_widget(
        Paragraph::new(message)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Center),
        layout.message_area,
    );
    render_dialog_actions(frame, layout.actions, actions);
    frame.render_widget(
        Paragraph::new("Click either button, or use Y / N · Enter confirms · Esc cancels")
            .style(Style::new().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        layout.hint_row,
    );
}

/// Paints button-sized backgrounds so actions read as clickable controls
/// instead of underlined letters embedded in prose.
pub fn render_dialog_actions(
    frame: &mut Frame,
    layout: DialogActionLayout,
    actions: DialogActions<'_>,
) {
    render_dialog_button(frame, layout.cancel_button, actions.cancel);
    render_dialog_button(frame, layout.confirm_button, actions.confirm);
}

/// Renders one centered action label using a dark-theme-safe tone.
fn render_dialog_button(frame: &mut Frame, area: Rect, button: DialogButton<'_>) {
    let style = match button.tone {
        DialogButtonTone::Neutral => Style::new()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
        DialogButtonTone::Primary => Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        DialogButtonTone::Danger => Style::new()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", button.key_hint),
                style.add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled(button.label, style),
        ]))
        .style(style)
        .alignment(Alignment::Center),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Flattens the test backend into text so visible action labels can be
    /// asserted without depending on terminal escape sequences.
    fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn confirmation_layout_exposes_two_distinct_mouse_targets_on_small_screens() {
        let screen = Rect::new(0, 0, 32, 8);
        let layout = confirm_dialog_layout(screen);

        assert!(screen.contains(layout.popup.as_position()));
        assert!(layout.popup.right() <= screen.right());
        assert!(layout.popup.bottom() <= screen.bottom());
        assert!(!layout.actions.cancel_button.is_empty());
        assert!(!layout.actions.confirm_button.is_empty());
        assert!(layout.actions.cancel_button.right() <= layout.actions.confirm_button.x);
        assert_eq!(
            layout
                .actions
                .action_at(layout.actions.cancel_button.as_position()),
            Some(DialogAction::Cancel)
        );
        assert_eq!(
            layout
                .actions
                .action_at(layout.actions.confirm_button.as_position()),
            Some(DialogAction::Confirm)
        );
    }

    #[test]
    fn destructive_confirmation_renders_aerated_named_buttons_with_dark_safe_tones() {
        let screen = Rect::new(0, 0, 100, 30);
        let layout = confirm_dialog_layout(screen);
        let mut terminal = Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();

        terminal
            .draw(|frame| {
                render_confirm(
                    frame,
                    screen,
                    "Close this item?",
                    "Close the selected group and all of its children?",
                    DialogActions::confirmation(
                        "Keep open",
                        DialogButtonTone::Neutral,
                        "Close",
                        DialogButtonTone::Danger,
                    ),
                );
            })
            .unwrap();

        let text = rendered_text(&terminal);
        assert!(text.contains("Keep open"));
        assert!(text.contains("Close"));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell(layout.actions.cancel_button.as_position())
                .unwrap()
                .bg,
            Color::DarkGray
        );
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell(layout.actions.confirm_button.as_position())
                .unwrap()
                .bg,
            Color::Red
        );
    }

    #[test]
    fn text_prompt_renders_clickable_cancel_and_primary_actions() {
        let screen = Rect::new(0, 0, 100, 30);
        let layout = text_prompt_dialog_layout(screen);
        let state = TextPromptState::new("new title");
        let mut terminal = Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();

        terminal
            .draw(|frame| {
                render_text_prompt(frame, screen, "Rename", &state, "Rename");
            })
            .unwrap();

        let text = rendered_text(&terminal);
        assert!(text.contains("Cancel"));
        assert!(text.contains("Rename"));
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell(layout.actions.confirm_button.as_position())
                .unwrap()
                .bg,
            Color::Cyan
        );
    }
}
