//! The kernel's popup host (foundation contract section 15.4).
//!
//! A client-side plugin opens a popup with the `open-view` intent and describes it with a view
//! whose root is a `dialog` widget. This module draws that view with the kernel's own modal
//! chrome and turns keys and clicks into `ui-action` events for the plugin. It understands the
//! widgets a dialog needs: message lines, `paragraph`, `choice` drawn as radio rows,
//! `check-list`, and the two editable rows `text-input` and `number` (gap G8). The plugin keeps
//! every choice and check value; the kernel keeps the cursor and, after the first draw, the text
//! and caret of an input and the value of a number, exactly as a panel view does
//! (`ui_state`). Escape and a click outside always close the popup here, whatever the plugin
//! does, so a stuck or trapped plugin can never hold the screen.
//!
//! Content longer than the screen scrolls: the rows follow the cursor, and the wheel, the
//! scrollbar column, PageUp/PageDown, Home/End (and the arrows on a text-only panel such as a
//! briefing, a queue or a help text) scroll them independently until the cursor moves again.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ilium_plugin_api::generated::types::ui::{NumberSpec, TextInputSpec};
use ilium_plugin_api::generated::types::{
    ActionKind, ActionValue, ButtonRole, Constraint, DialogButton, Span as PluginSpan, TriState,
    UiAction, UiTree, ViewTarget, Widget,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use super::ui_state::{
    format_number, DueChange, EditOutcome, NumberEdit, NumberOutcome, TextEdit, WidgetStates,
};
use super::view::{cells, clean, fit};
use crate::app::{App, Mode};
use crate::value_control::leader_span;
use crate::{modal, theme};

/// Width of a popup whose dialog does not ask for a fixed one.
const DEFAULT_WIDTH: u16 = 64;
/// Rows of chrome around the content: two border rows, one blank row, one button row.
const CHROME_ROWS: u16 = 4;
/// The key the kernel reports dialog buttons under when the root node has no key.
const DIALOG_KEY: &str = "dialog";
/// Cells of the cursor marker in front of every row (`›` and a blank).
const MARKER_CELLS: u16 = 2;
/// Narrowest field of a text input.
const MIN_FIELD_CELLS: u16 = 10;

/// What the kernel remembers about the open popup: whose it is, where the cursor sits, and the
/// edit state of its inputs and numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct PopupState {
    pub plugin: String,
    pub popup: String,
    /// Index into the focusable rows (choice options, check items, inputs and numbers).
    pub row: usize,
    /// Index of the focused button; `None` until the first draw picks the primary one.
    pub button: Option<usize>,
    /// Key of the widget the cursor was last placed in. When the plugin shows another page
    /// the key changes and the cursor starts again at the top.
    pub page: Option<String>,
    /// Text, caret and number state of the editable rows, by widget key.
    pub(crate) edits: WidgetStates,
    /// First content row the user scrolled to (wheel, scrollbar, PageUp/PageDown, or Up/Down
    /// on a popup without focusable rows); `None` keeps the cursor row in view. Moving the
    /// cursor goes back to following it.
    pub(crate) scroll: Option<usize>,
}

impl PopupState {
    pub(crate) fn new(plugin: String, popup: String) -> Self {
        Self {
            plugin,
            popup,
            row: 0,
            button: None,
            page: None,
            edits: WidgetStates::default(),
            scroll: None,
        }
    }
}

/// One content row of a popup.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Row {
    Text(String),
    Choice {
        key: String,
        index: u32,
        label: String,
        selected: bool,
    },
    Check {
        key: String,
        id: String,
        label: String,
        state: TriState,
        disabled: bool,
    },
    /// A `text-input`: typed into while the cursor is on it.
    Input {
        key: String,
        spec: TextInputSpec,
    },
    /// A `number`: stepped with Left and Right, typed after a digit or `*`.
    Number {
        key: String,
        spec: NumberSpec,
    },
}

impl Row {
    fn is_focusable(&self) -> bool {
        !matches!(self, Row::Text(_))
    }

    fn key(&self) -> Option<&str> {
        match self {
            Row::Text(_) => None,
            Row::Choice { key, .. }
            | Row::Check { key, .. }
            | Row::Input { key, .. }
            | Row::Number { key, .. } => Some(key),
        }
    }
}

/// A popup view reduced to what the kernel draws.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PopupModel {
    pub title: String,
    pub dialog_key: String,
    pub rows: Vec<Row>,
    pub buttons: Vec<DialogButton>,
    pub width: u16,
}

fn plain(spans: &[PluginSpan]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// Reads a popup view. `None` when the root is not a `dialog`.
pub(crate) fn model(tree: &UiTree) -> Option<PopupModel> {
    let root = tree.nodes.get(usize::try_from(tree.root).ok()?)?;
    let Widget::Dialog(dialog) = &root.widget else {
        return None;
    };
    let mut rows: Vec<Row> = dialog
        .message
        .iter()
        .flatten()
        .map(|line| Row::Text(plain(line)))
        .collect();
    for child in &root.children {
        let Some(node) = usize::try_from(*child)
            .ok()
            .and_then(|index| tree.nodes.get(index))
        else {
            continue;
        };
        // An editable row needs a key of its own for its state; an unkeyed one is keyed by
        // its node index, which stays stable while the plugin keeps the same tree shape.
        let key = node.key.clone().unwrap_or_else(|| format!("#{child}"));
        match &node.widget {
            Widget::Paragraph(paragraph) => {
                rows.extend(paragraph.lines.iter().map(|line| Row::Text(plain(line))));
            }
            Widget::Choice(choice) => {
                rows.extend(
                    choice
                        .options
                        .iter()
                        .zip(0u32..)
                        .map(|(label, index)| Row::Choice {
                            key: key.clone(),
                            index,
                            label: label.clone(),
                            selected: index == choice.selected,
                        }),
                );
            }
            Widget::CheckList(list) => {
                rows.extend(list.items.iter().map(|item| Row::Check {
                    key: key.clone(),
                    id: item.id.clone(),
                    label: plain(&item.label),
                    state: item.state,
                    disabled: item.disabled_reason.is_some(),
                }));
            }
            Widget::TextInput(spec) => rows.push(Row::Input {
                key,
                spec: spec.clone(),
            }),
            Widget::Number(spec) => rows.push(Row::Number {
                key,
                spec: spec.clone(),
            }),
            other => {
                tracing::debug!("popup widget not drawn by the kernel popup host: {other:?}");
            }
        }
    }
    let width = match dialog.width {
        Constraint::Length(width) | Constraint::Min(width) | Constraint::Max(width) => width,
        _ => DEFAULT_WIDTH,
    };
    Some(PopupModel {
        title: plain(&dialog.title),
        dialog_key: root.key.clone().unwrap_or_else(|| DIALOG_KEY.to_owned()),
        rows,
        buttons: dialog.buttons.clone(),
        width: width.max(24),
    })
}

/// Where everything of a popup lands on screen; drawing and hit-testing share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub popup: Rect,
    pub inner: Rect,
    /// The content rows shown: indexes `start..end` into `PopupModel::rows`.
    pub start: usize,
    pub end: usize,
    pub buttons: Vec<Rect>,
    /// The scrollbar column at the right of the content rows, when they do not all fit.
    pub bar: Option<Rect>,
}

impl Geometry {
    /// The cells of a shown content row after its cursor marker; `None` for a row not shown.
    fn row_area(&self, row: usize) -> Option<Rect> {
        if row < self.start || row >= self.end {
            return None;
        }
        let offset = u16::try_from(row - self.start).ok()?;
        Some(Rect::new(
            self.inner.x.saturating_add(MARKER_CELLS),
            self.inner.y.saturating_add(offset),
            self.content().width.saturating_sub(MARKER_CELLS),
            1,
        ))
    }

    /// The cells of the content rows: the inner area less the blank row, the button row and
    /// the scrollbar column.
    fn content(&self) -> Rect {
        Rect::new(
            self.inner.x,
            self.inner.y,
            self.inner
                .width
                .saturating_sub(self.bar.map_or(0, |bar| bar.width)),
            self.inner.height.saturating_sub(2),
        )
    }

    /// The first row a scrolled popup can start at.
    fn max_start(&self, rows: usize) -> usize {
        rows.saturating_sub(self.end - self.start)
    }
}

/// How an editable row splits its cells: the label room on the left, the field on the right.
/// Drawing, the caret and the click test all read this one split.
fn input_split(area: Rect) -> (u16, Rect) {
    let field_width = (area.width / 2).max(MIN_FIELD_CELLS).min(area.width);
    let label_room = area.width - field_width;
    (
        label_room,
        Rect::new(area.x + label_room, area.y, field_width, area.height),
    )
}

/// The text a number row shows: the typed entry with a caret, or the formatted value.
fn number_text(spec: &NumberSpec, edit: &NumberEdit) -> String {
    match &edit.typed {
        Some(typed) => format!("‹ {typed}_ ›"),
        None => format!("‹ {} ›", clean(&format_number(spec, edit.value))),
    }
}

/// The stepper field of a number row, right-aligned in its cells.
fn number_field(area: Rect, shown: &str) -> Rect {
    let width = u16::try_from(cells(shown))
        .unwrap_or(u16::MAX)
        .min(area.width);
    Rect::new(area.right() - width, area.y, width, area.height)
}

fn focusable_rows(model: &PopupModel) -> Vec<usize> {
    model
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_focusable())
        .map(|(index, _)| index)
        .collect()
}

/// The row under the cursor.
fn focused_row<'a>(model: &'a PopupModel, state: &PopupState) -> Option<&'a Row> {
    let index = *focusable_rows(model).get(state.row)?;
    model.rows.get(index)
}

/// Brings the cursor in line with the view the plugin now shows: back to the top (and to the
/// primary button) when the page changed, and never past the last row.
pub(crate) fn sync(model: &PopupModel, state: &mut PopupState) {
    let page = model.rows.iter().find_map(Row::key).map(str::to_owned);
    if state.page != page {
        state.page = page;
        state.row = 0;
        state.button = None;
        state.scroll = None;
    }
    state.row = state.row.min(focusable_rows(model).len().saturating_sub(1));
    // A typed number entry is parsed with the spec of the row it belongs to.
    for row in &model.rows {
        if let Row::Number { key, spec } = row {
            state.edits.number_specs.insert(key.clone(), spec.clone());
        }
    }
}

fn button_label(button: &DialogButton) -> String {
    format!("[ {} ]", button.label)
}

fn width_of(text: &str) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap_or(u16::MAX)
}

pub(crate) fn geometry(model: &PopupModel, state: &PopupState, screen: Rect) -> Geometry {
    let wanted_rows = u16::try_from(model.rows.len()).unwrap_or(u16::MAX);
    let popup =
        modal::centered_fixed_rect(model.width, wanted_rows.saturating_add(CHROME_ROWS), screen);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    // The last two inner rows are the blank row and the button row.
    let capacity = usize::from(inner.height.saturating_sub(2));
    let cursor = focusable_rows(model).get(state.row).copied().unwrap_or(0);
    let max_start = model.rows.len().saturating_sub(capacity);
    let start = match state.scroll {
        Some(top) => top.min(max_start),
        None => cursor.saturating_sub(capacity / 2).min(max_start),
    };
    let end = (start + capacity).min(model.rows.len());
    let bar = (model.rows.len() > capacity && capacity > 0 && inner.width > MARKER_CELLS + 1).then(
        || {
            Rect::new(
                inner.right().saturating_sub(1),
                inner.y,
                1,
                u16::try_from(capacity).unwrap_or(u16::MAX),
            )
        },
    );
    let button_row = inner.bottom().saturating_sub(1);
    let mut column = inner.x.saturating_add(1);
    let mut buttons = Vec::with_capacity(model.buttons.len());
    for button in &model.buttons {
        let width = width_of(&button_label(button)).min(inner.right().saturating_sub(column));
        buttons.push(Rect::new(column, button_row, width, u16::from(width > 0)));
        column = column.saturating_add(width).saturating_add(1);
    }
    Geometry {
        popup,
        inner,
        start,
        end,
        buttons,
        bar,
    }
}

/// Scrolls the content rows by `delta` rows. Returns whether the shown rows moved.
fn scroll_by(model: &PopupModel, state: &mut PopupState, screen: Rect, delta: i64) -> bool {
    let geometry = geometry(model, state, screen);
    let max_start = geometry.max_start(model.rows.len());
    let target = i64::try_from(geometry.start)
        .unwrap_or(i64::MAX)
        .saturating_add(delta)
        .clamp(0, i64::try_from(max_start).unwrap_or(i64::MAX));
    let target = usize::try_from(target).unwrap_or(max_start);
    state.scroll = Some(target);
    target != geometry.start
}

/// The thumb of a scrollbar `track` rows long over `total` rows of which `shown` start at
/// `start`: its first row and its length.
fn thumb(track: u16, total: usize, shown: usize, start: usize) -> (u16, u16) {
    let track_rows = usize::from(track);
    let total = total.max(track_rows).max(1);
    let length = (track_rows * shown.min(total) / total).clamp(1, track_rows.max(1));
    let travel = track_rows - length;
    let max_start = total.saturating_sub(shown).max(1);
    let first = start.min(max_start) * travel / max_start;
    (
        u16::try_from(first).unwrap_or(0),
        u16::try_from(length).unwrap_or(1),
    )
}

/// The wheel over a popup scrolls its content rows three per notch; elsewhere it does nothing.
pub(crate) fn wheel_reaction(
    model: &PopupModel,
    state: &mut PopupState,
    screen: Rect,
    position: Position,
    notches: i64,
) -> Reaction {
    sync(model, state);
    if geometry(model, state, screen).popup.contains(position) {
        scroll_by(model, state, screen, notches.saturating_mul(3));
    }
    Reaction::Stay
}

fn focused_button(model: &PopupModel, state: &PopupState) -> usize {
    state
        .button
        .filter(|index| *index < model.buttons.len())
        .or_else(|| {
            model
                .buttons
                .iter()
                .position(|button| button.role == ButtonRole::Primary)
        })
        .unwrap_or(0)
}

fn action(
    state: &PopupState,
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> UiAction {
    UiAction {
        view: ViewTarget::Popup(state.popup.clone()),
        key: key.to_owned(),
        action_id: action_id.to_owned(),
        kind,
        value,
    }
}

/// The `changed` action of an edit the debounce (or a focus change) released.
fn due_action(state: &PopupState, change: DueChange) -> UiAction {
    match change {
        DueChange::Text(key, text) => action(
            state,
            &key,
            "",
            ActionKind::Changed,
            Some(ActionValue::Text(text)),
        ),
        DueChange::Number(key, value) => action(
            state,
            &key,
            "",
            ActionKind::Changed,
            Some(ActionValue::Number(value)),
        ),
    }
}

/// Sends the edits whose debounce has run out at `now`.
fn release_due(state: &mut PopupState, now: Instant, out: &mut Vec<UiAction>) {
    let released = state.edits.take_due(now);
    out.extend(released.into_iter().map(|change| due_action(state, change)));
}

/// Sends the unsent edit of one row now, as when the cursor leaves it.
fn flush_row(state: &mut PopupState, key: &str, out: &mut Vec<UiAction>) {
    if let Some(change) = state.edits.flush(key) {
        out.push(due_action(state, change));
    }
}

/// Sends every unsent edit now, so a button never reaches the plugin before the values it
/// acts on.
fn flush_all(state: &mut PopupState, out: &mut Vec<UiAction>) {
    let keys: Vec<String> = state
        .edits
        .texts
        .keys()
        .chain(state.edits.numbers.keys())
        .cloned()
        .collect();
    for key in keys {
        flush_row(state, &key, out);
    }
}

/// The action a choice or check row sends when it is chosen with Space, a click, or by moving
/// onto a choice. Editable rows send their own actions as they are edited.
fn row_action(model: &PopupModel, state: &PopupState, pick: bool) -> Option<UiAction> {
    match focused_row(model, state)? {
        Row::Choice {
            key,
            index,
            selected,
            ..
        } if !selected => Some(action(
            state,
            key,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(*index)),
        )),
        Row::Check {
            key,
            id,
            state: checked,
            disabled: false,
            ..
        } if pick => {
            let next = if *checked == TriState::On {
                TriState::Off
            } else {
                TriState::On
            };
            Some(action(
                state,
                key,
                "",
                ActionKind::Toggled,
                Some(ActionValue::ItemState((id.clone(), next))),
            ))
        }
        _ => None,
    }
}

fn button_action(model: &PopupModel, state: &PopupState, index: usize) -> Option<UiAction> {
    let button = model.buttons.get(index)?;
    Some(action(
        state,
        &model.dialog_key,
        &button.id,
        ActionKind::Activated,
        None,
    ))
}

/// What one key or click on a popup amounts to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reaction {
    /// Nothing for the plugin; the cursor or an edit may have moved.
    Stay,
    /// The plugin gets these actions, in order; the popup stays until the plugin closes it.
    Send(Vec<UiAction>),
    /// The kernel closes the popup and tells the plugin.
    Dismiss,
}

fn finish(out: Vec<UiAction>) -> Reaction {
    if out.is_empty() {
        Reaction::Stay
    } else {
        Reaction::Send(out)
    }
}

/// A key on a focused text input. `None` when the input does not use the key, so the dialog
/// keys (Up, Down, Tab, Escape, Enter without `submit-on-enter`) still work.
fn input_key(
    state: &mut PopupState,
    key: &str,
    spec: &TextInputSpec,
    event: KeyEvent,
    now: Instant,
    out: &mut Vec<UiAction>,
) -> Option<()> {
    let edit = state
        .edits
        .texts
        .entry(key.to_owned())
        .or_insert_with(|| TextEdit::seeded(&spec.initial));
    match edit.apply_key(event, spec, now) {
        EditOutcome::Ignored => None,
        EditOutcome::Moved => Some(()),
        EditOutcome::Edited => {
            release_due(state, now, out);
            Some(())
        }
        EditOutcome::Submitted => {
            let text = edit.text().to_owned();
            flush_row(state, key, out);
            out.push(action(
                state,
                key,
                "",
                ActionKind::Submitted,
                Some(ActionValue::Text(text)),
            ));
            Some(())
        }
    }
}

/// A key on a focused number. Enter belongs to the dialog's button unless a typed entry is
/// open, so a person can always confirm a dialog from a number row.
fn number_key(
    state: &mut PopupState,
    key: &str,
    spec: &NumberSpec,
    event: KeyEvent,
    now: Instant,
    out: &mut Vec<UiAction>,
) -> Option<()> {
    let edit = state
        .edits
        .numbers
        .entry(key.to_owned())
        .or_insert_with(|| NumberEdit::seeded(spec));
    if event.code == KeyCode::Enter && edit.typed.is_none() {
        return None;
    }
    match edit.apply_key(event, spec, now) {
        NumberOutcome::Ignored => None,
        NumberOutcome::Typing => Some(()),
        NumberOutcome::Changed(value) => {
            out.push(action(
                state,
                key,
                "",
                ActionKind::Changed,
                Some(ActionValue::Number(value)),
            ));
            Some(())
        }
    }
}

pub(crate) fn key_reaction(
    model: &PopupModel,
    state: &mut PopupState,
    screen: Rect,
    event: KeyEvent,
    now: Instant,
) -> Reaction {
    sync(model, state);
    let mut out = Vec::new();
    release_due(state, now, &mut out);
    // The editable row under the cursor takes the keys it edits with first.
    let handled = match focused_row(model, state) {
        Some(Row::Input { key, spec }) => input_key(state, key, spec, event, now, &mut out),
        Some(Row::Number { key, spec }) => number_key(state, key, spec, event, now, &mut out),
        _ => None,
    };
    if handled.is_some() {
        return finish(out);
    }
    let focusable = focusable_rows(model).len();
    let buttons = model.buttons.len();
    let current_button = focused_button(model, state);
    let plain_key = !event
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let page = i64::try_from(
        geometry(model, state, screen)
            .content()
            .height
            .saturating_sub(1)
            .max(1),
    )
    .unwrap_or(1);
    match event.code {
        KeyCode::Esc => return Reaction::Dismiss,
        // Without a row to move to, the arrows read the text: they scroll it.
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j')
            if plain_key && focusable == 0 =>
        {
            let down = matches!(event.code, KeyCode::Down | KeyCode::Char('j'));
            scroll_by(model, state, screen, if down { 1 } else { -1 });
        }
        KeyCode::PageDown => {
            scroll_by(model, state, screen, page);
        }
        KeyCode::PageUp => {
            scroll_by(model, state, screen, -page);
        }
        KeyCode::Home if plain_key => {
            scroll_by(model, state, screen, i64::MIN / 2);
        }
        KeyCode::End if plain_key => {
            scroll_by(model, state, screen, i64::MAX / 2);
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') if plain_key => {
            state.scroll = None;
            if let Some(key) = focused_row(model, state).and_then(Row::key) {
                flush_row(state, key, &mut out);
            }
            let down = matches!(event.code, KeyCode::Down | KeyCode::Char('j'));
            state.row = if down {
                (state.row + 1).min(focusable.saturating_sub(1))
            } else {
                state.row.saturating_sub(1)
            };
            out.extend(row_action(model, state, false));
        }
        KeyCode::Char(' ') if plain_key => out.extend(row_action(model, state, true)),
        KeyCode::Tab | KeyCode::Right if buttons > 0 => {
            state.button = Some((current_button + 1) % buttons);
        }
        KeyCode::BackTab | KeyCode::Left if buttons > 0 => {
            state.button = Some((current_button + buttons - 1) % buttons);
        }
        KeyCode::Enter => {
            flush_all(state, &mut out);
            out.extend(button_action(model, state, current_button));
        }
        _ => {}
    }
    finish(out)
}

/// A bracketed paste: one change of the focused text input; nothing on any other row.
pub(crate) fn paste_reaction(
    model: &PopupModel,
    state: &mut PopupState,
    pasted: &str,
    now: Instant,
) -> Reaction {
    sync(model, state);
    let mut out = Vec::new();
    release_due(state, now, &mut out);
    if let Some(Row::Input { key, spec }) = focused_row(model, state) {
        let edited = state
            .edits
            .texts
            .entry(key.clone())
            .or_insert_with(|| TextEdit::seeded(&spec.initial))
            .paste(pasted, spec, now);
        if edited {
            release_due(state, now, &mut out);
        }
    }
    finish(out)
}

/// A click on a number row: the `‹` cell steps down, the `›` cell steps up.
fn number_click(
    state: &mut PopupState,
    key: &str,
    spec: &NumberSpec,
    area: Rect,
    column: u16,
    out: &mut Vec<UiAction>,
) {
    let edit = state
        .edits
        .numbers
        .entry(key.to_owned())
        .or_insert_with(|| NumberEdit::seeded(spec));
    let field = number_field(area, &number_text(spec, edit));
    let direction = if field.width == 0 {
        0
    } else if column == field.x {
        -1
    } else if column + 1 == field.right() {
        1
    } else {
        0
    };
    if direction == 0 {
        return;
    }
    if let NumberOutcome::Changed(value) = edit.step(spec, direction) {
        out.push(action(
            state,
            key,
            "",
            ActionKind::Changed,
            Some(ActionValue::Number(value)),
        ));
    }
}

pub(crate) fn click_reaction(
    model: &PopupModel,
    state: &mut PopupState,
    screen: Rect,
    position: Position,
    now: Instant,
) -> Reaction {
    sync(model, state);
    let geometry = geometry(model, state, screen);
    if !geometry.popup.contains(position) {
        return Reaction::Dismiss;
    }
    let mut out = Vec::new();
    release_due(state, now, &mut out);
    if let Some(index) = geometry
        .buttons
        .iter()
        .position(|button| button.contains(position))
    {
        state.button = Some(index);
        flush_all(state, &mut out);
        out.extend(button_action(model, state, index));
        return finish(out);
    }
    if let Some(bar) = geometry.bar.filter(|bar| bar.contains(position)) {
        // A click on the scrollbar shows the rows at that share of the content.
        let max_start = geometry.max_start(model.rows.len());
        let travel = usize::from(bar.height.saturating_sub(1)).max(1);
        let row = usize::from(position.y - bar.y);
        state.scroll = Some((row * max_start).div_ceil(travel).min(max_start));
        return finish(out);
    }
    if !geometry.inner.contains(position) {
        return finish(out);
    }
    let row = geometry.start + usize::from(position.y.saturating_sub(geometry.inner.y));
    if row >= geometry.end {
        return finish(out);
    }
    let Some(focus) = focusable_rows(model).iter().position(|index| *index == row) else {
        return finish(out);
    };
    if focus != state.row {
        if let Some(key) = focused_row(model, state).and_then(Row::key) {
            flush_row(state, key, &mut out);
        }
    }
    state.row = focus;
    let area = geometry.row_area(row).unwrap_or_default();
    match model.rows.get(row) {
        Some(Row::Input { key, spec }) => {
            let (_, field) = input_split(area);
            if field.contains(position) {
                state
                    .edits
                    .texts
                    .entry(key.clone())
                    .or_insert_with(|| TextEdit::seeded(&spec.initial))
                    .place_caret(spec, usize::from(field.width), position.x - field.x);
            }
        }
        Some(Row::Number { key, spec }) => {
            number_click(state, key, spec, area, position.x, &mut out);
        }
        _ => out.extend(row_action(model, state, true)),
    }
    finish(out)
}

/// The line of a text input row: label, leader and an underlined field showing the edit
/// state (the plugin's `initial` until the first key). Returns the line and the caret column
/// inside the field.
fn input_line(spec: &TextInputSpec, edit: &TextEdit, area: Rect) -> (Vec<Span<'static>>, u16) {
    let (label_room, field) = input_split(area);
    let label = fit(&clean(&spec.label), usize::from(label_room));
    let gap = usize::from(label_room).saturating_sub(cells(&label));
    let layout = edit.layout(spec, usize::from(field.width));
    let mut base = Style::new().add_modifier(Modifier::UNDERLINED);
    if !edit.is_valid(spec) {
        base = base.fg(Color::Red);
    }
    let mut spans = vec![Span::raw(label), leader_span(gap)];
    let mut used = 0usize;
    match spec.placeholder.as_deref() {
        Some(hint) if edit.text().is_empty() => {
            let hint = fit(&clean(hint), usize::from(field.width));
            used = cells(&hint);
            spans.push(Span::styled(hint, base.add_modifier(Modifier::DIM)));
        }
        _ => {
            for cell in &layout.cells {
                used += cells(&cell.text);
                let style = if cell.selected {
                    base.add_modifier(Modifier::REVERSED)
                } else {
                    base
                };
                spans.push(Span::styled(cell.text.clone(), style));
            }
        }
    }
    let width = usize::from(field.width);
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), base));
    }
    (spans, layout.caret_column)
}

/// The line of a number row: label, leader and the `‹ value ›` stepper, right-aligned.
fn number_line(spec: &NumberSpec, edit: &NumberEdit, area: Rect) -> Vec<Span<'static>> {
    let shown = fit(&number_text(spec, edit), usize::from(area.width));
    let field = number_field(area, &shown);
    let label_room = field.x - area.x;
    let label = fit(&clean(&spec.label), usize::from(label_room));
    let gap = usize::from(label_room).saturating_sub(cells(&label));
    vec![Span::raw(label), leader_span(gap), Span::raw(shown)]
}

/// Draws the open popup. Nothing is drawn until the plugin's first view arrived.
pub(crate) fn draw(frame: &mut Frame, screen: Rect, state: &PopupState, app: &App) {
    let Some(model) = app
        .wasm_plugins
        .as_ref()
        .and_then(|plugins| plugins.popup_tree(&state.plugin, &state.popup))
        .as_ref()
        .and_then(model)
    else {
        return;
    };
    let mut synced = state.clone();
    sync(&model, &mut synced);
    let state = &synced;
    let geometry = geometry(&model, state, screen);
    frame.render_widget(Clear, geometry.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title(&model.title)),
        geometry.popup,
    );
    let cursor = focusable_rows(&model).get(state.row).copied();
    let mut caret: Option<Position> = None;
    let lines: Vec<Line> = model.rows[geometry.start..geometry.end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let index = geometry.start + offset;
            let is_cursor = cursor == Some(index);
            let marker = if is_cursor { "›" } else { " " };
            let style = if is_cursor {
                theme::selected_style()
            } else {
                Style::new()
            };
            let area = geometry.row_area(index).unwrap_or_default();
            match row {
                Row::Text(text) => Line::from(text.clone()),
                Row::Choice {
                    label, selected, ..
                } => {
                    let radio = if *selected { "(•)" } else { "( )" };
                    Line::from(Span::styled(format!("{marker} {radio} {label}"), style))
                }
                Row::Check {
                    label,
                    state: checked,
                    disabled,
                    ..
                } => {
                    let checkbox = match checked {
                        TriState::On => "[x]",
                        TriState::Mixed => "[-]",
                        TriState::Off => "[ ]",
                    };
                    let style = if *disabled {
                        style.add_modifier(Modifier::DIM)
                    } else {
                        style
                    };
                    Line::from(Span::styled(format!("{marker} {checkbox} {label}"), style))
                }
                Row::Input { key, spec } => {
                    let seeded;
                    let edit = match state.edits.texts.get(key) {
                        Some(edit) => edit,
                        None => {
                            seeded = TextEdit::seeded(&spec.initial);
                            &seeded
                        }
                    };
                    let (spans, caret_column) = input_line(spec, edit, area);
                    if is_cursor && area.width > 0 {
                        let (_, field) = input_split(area);
                        caret = Some(Position::new(field.x.saturating_add(caret_column), field.y));
                    }
                    let mut line = vec![Span::styled(format!("{marker} "), style)];
                    line.extend(spans);
                    Line::from(line)
                }
                Row::Number { key, spec } => {
                    let seeded;
                    let edit = match state.edits.numbers.get(key) {
                        Some(edit) => edit,
                        None => {
                            seeded = NumberEdit::seeded(spec);
                            &seeded
                        }
                    };
                    let mut line = vec![Span::styled(format!("{marker} "), style)];
                    line.extend(number_line(spec, edit, area));
                    Line::from(line)
                }
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), geometry.content());
    if let Some(bar) = geometry.bar {
        let (first, length) = thumb(
            bar.height,
            model.rows.len(),
            geometry.end - geometry.start,
            geometry.start,
        );
        let glyphs: Vec<Line> = (0..bar.height)
            .map(|row| {
                if row >= first && row < first + length {
                    Line::from("┃")
                } else {
                    Line::styled("│", Style::new().add_modifier(Modifier::DIM))
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(glyphs), bar);
    }
    if let Some(caret) = caret {
        frame.set_cursor_position(caret);
    }
    let focused = focused_button(&model, state);
    for (index, (button, area)) in model.buttons.iter().zip(&geometry.buttons).enumerate() {
        let style = if index == focused {
            theme::selected_style()
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        frame.render_widget(Paragraph::new(button_label(button)).style(style), *area);
    }
}

impl App {
    fn plugin_popup_model(&self, state: &PopupState) -> Option<PopupModel> {
        self.wasm_plugins
            .as_ref()
            .and_then(|plugins| plugins.popup_tree(&state.plugin, &state.popup))
            .as_ref()
            .and_then(model)
    }

    fn apply_popup_reaction(&mut self, state: PopupState, reaction: Reaction) {
        match reaction {
            Reaction::Stay => self.mode = Mode::PluginPopup(state),
            Reaction::Send(actions) => {
                if let Some(plugins) = self.wasm_plugins.as_ref() {
                    plugins.popup_actions(&state.plugin, actions, self.popup_screen());
                }
                self.mode = Mode::PluginPopup(state);
            }
            Reaction::Dismiss => {
                if let Some(plugins) = self.wasm_plugins.as_ref() {
                    plugins.dismiss_popup(&state.plugin, &state.popup);
                }
                self.close_plugin_popup_mode();
            }
        }
    }

    /// Leaves the popup screen: back to the Settings page that asked for the popup, or to the
    /// normal view.
    pub(crate) fn close_plugin_popup_mode(&mut self) {
        self.mode = match self.settings_suspended_for_popup.take() {
            Some(settings) => Mode::Settings(settings),
            None => Mode::Normal,
        };
    }

    pub(crate) fn popup_screen(&self) -> (u16, u16) {
        (
            self.layout.screen_area.width,
            self.layout.screen_area.height,
        )
    }

    /// One key press while a plugin popup is open. The caller took the state out of `mode`.
    pub(crate) fn plugin_popup_key(&mut self, mut state: PopupState, key: KeyEvent) {
        let reaction = match self.plugin_popup_model(&state) {
            Some(model) => key_reaction(
                &model,
                &mut state,
                self.layout.screen_area,
                key,
                Instant::now(),
            ),
            // No view yet: only Escape does anything.
            None if key.code == KeyCode::Esc => Reaction::Dismiss,
            None => Reaction::Stay,
        };
        self.apply_popup_reaction(state, reaction);
    }

    /// A bracketed paste while a plugin popup is open. The caller took the state out of `mode`.
    pub(crate) fn plugin_popup_paste(&mut self, mut state: PopupState, pasted: &str) {
        let reaction = match self.plugin_popup_model(&state) {
            Some(model) => paste_reaction(&model, &mut state, pasted, Instant::now()),
            None => Reaction::Stay,
        };
        self.apply_popup_reaction(state, reaction);
    }

    /// One mouse event while a plugin popup is open. The caller took the state out of `mode`.
    pub(crate) fn plugin_popup_mouse(&mut self, mut state: PopupState, mouse: MouseEvent) {
        let position = Position::new(mouse.column, mouse.row);
        let screen = self.layout.screen_area;
        let notches = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ => 0,
        };
        if notches != 0 {
            let reaction = match self.plugin_popup_model(&state) {
                Some(model) => wheel_reaction(&model, &mut state, screen, position, notches),
                None => Reaction::Stay,
            };
            self.apply_popup_reaction(state, reaction);
            return;
        }
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.mode = Mode::PluginPopup(state);
            return;
        }
        let reaction = match self.plugin_popup_model(&state) {
            Some(model) => click_reaction(&model, &mut state, screen, position, Instant::now()),
            None => Reaction::Dismiss,
        };
        self.apply_popup_reaction(state, reaction);
    }

    /// Sends the popup edits whose debounce ran out. Returns whether anything was sent.
    fn tick_plugin_popup_edits(&mut self, now: Instant) -> bool {
        let Mode::PluginPopup(state) = &mut self.mode else {
            return false;
        };
        if state.edits.next_due().is_none() {
            return false;
        }
        let mut actions = Vec::new();
        release_due(state, now, &mut actions);
        if actions.is_empty() {
            return false;
        }
        let plugin = state.plugin.clone();
        let screen = self.popup_screen();
        if let Some(plugins) = self.wasm_plugins.as_ref() {
            plugins.popup_actions(&plugin, actions, screen);
        }
        true
    }

    /// Sends every plugin edit (popup and shown panel view) whose debounce ran out. Called
    /// once per turn of the event loop. Returns whether anything was sent.
    pub(crate) fn tick_plugin_edits(&mut self, now: Instant) -> bool {
        let popup = self.tick_plugin_popup_edits(now);
        let panel = self.tick_plugin_view_edits(now);
        let panes = self.tick_plugin_pane_edits(now);
        popup || panel || panes
    }

    /// How long until the next unsent plugin edit falls due, so the event loop wakes for it;
    /// `None` when nothing waits.
    pub(crate) fn plugin_edit_delay(&self, now: Instant) -> Option<Duration> {
        let popup = match &self.mode {
            Mode::PluginPopup(state) => state.edits.next_due(),
            _ => None,
        };
        let panel = self
            .shown_plugin_view()
            .and_then(|_| super::view::next_due(&self.plugin_view_cursor));
        popup
            .into_iter()
            .chain(panel)
            .chain(self.plugin_pane_edit_due())
            .min()
            .map(|due| due.saturating_duration_since(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_api::generated::types::ui::NumberStyle;
    use ilium_plugin_api::generated::types::{
        CheckItem, CheckListSpec, DialogKind, DialogSpec, NodeFlags, UiNode,
    };

    fn node(key: Option<&str>, widget: Widget, children: Vec<u32>) -> UiNode {
        UiNode {
            key: key.map(str::to_owned),
            widget,
            children,
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }
    }

    fn span(text: &str) -> Vec<PluginSpan> {
        vec![PluginSpan {
            text: text.to_owned(),
            style: Default::default(),
        }]
    }

    fn button(id: &str, role: ButtonRole, is_cancel: bool) -> DialogButton {
        DialogButton {
            id: id.to_owned(),
            label: id.to_owned(),
            role,
            is_cancel,
        }
    }

    fn dialog(message: Option<&str>) -> DialogSpec {
        DialogSpec {
            title: span("Add panes to split"),
            kind: DialogKind::Plain,
            buttons: vec![
                button("cancel", ButtonRole::Secondary, true),
                button("create", ButtonRole::Primary, false),
            ],
            width: Constraint::Length(72),
            height: Constraint::Min(0),
            message: message.map(|text| vec![span(text)]),
        }
    }

    fn picker() -> UiTree {
        let list = CheckListSpec {
            items: ["1", "2"]
                .iter()
                .map(|id| CheckItem {
                    id: (*id).to_owned(),
                    label: span(id),
                    state: TriState::Off,
                    disabled_reason: None,
                    children: Vec::new(),
                })
                .collect(),
            all_toggle: false,
            max_height: None,
        };
        UiTree {
            version: 1,
            nodes: vec![
                node(
                    Some("dialog"),
                    Widget::Dialog(dialog(Some("Select up to four panes (0/4)."))),
                    vec![1],
                ),
                node(Some("members"), Widget::CheckList(list), Vec::new()),
            ],
            root: 0,
        }
    }

    /// A rename dialog: one text input and one number, no message.
    fn rename(debounce_ms: u32, submit_on_enter: bool) -> UiTree {
        let input = TextInputSpec {
            label: "Name".to_owned(),
            initial: "ab".to_owned(),
            debounce_ms,
            submit_on_enter,
            ..TextInputSpec::default()
        };
        let number = NumberSpec {
            label: "Columns".to_owned(),
            value: 2.0,
            minimum: 1.0,
            maximum: 4.0,
            step: 1.0,
            unit: None,
            style: NumberStyle::Stepper,
            steps: Vec::new(),
            special_values: Vec::new(),
            scale: 1,
            // A typed entry waits for Enter rather than going out while the digits are typed.
            debounce_ms: 60_000,
        };
        UiTree {
            version: 1,
            nodes: vec![
                node(Some("dialog"), Widget::Dialog(dialog(None)), vec![1, 2]),
                node(Some("name"), Widget::TextInput(input), Vec::new()),
                node(Some("columns"), Widget::Number(number), Vec::new()),
            ],
            root: 0,
        }
    }

    fn state() -> PopupState {
        PopupState::new("ilium.core.layout".to_owned(), "new-split".to_owned())
    }

    /// The screen the key tests react on; tall enough that small dialogs never scroll.
    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 40,
    };

    /// A text-only panel (a briefing or help text): many message lines and the two buttons.
    fn briefing(lines: usize) -> UiTree {
        let mut spec = dialog(None);
        spec.message = Some(
            (0..lines)
                .map(|index| span(&format!("line {index}")))
                .collect(),
        );
        UiTree {
            version: 1,
            root: 0,
            nodes: vec![node(Some("briefing"), Widget::Dialog(spec), Vec::new())],
        }
    }

    #[test]
    fn a_long_text_popup_scrolls_with_keys_the_wheel_and_its_scrollbar() {
        let model = model(&briefing(30)).expect("dialog");
        let mut state = state();
        // Ten rows of screen: eight inner rows, six of them for content.
        let screen = Rect::new(0, 0, 80, 10);
        let first = geometry(&model, &state, screen);
        assert_eq!((first.start, first.end), (0, 6));
        let bar = first
            .bar
            .expect("the rows do not fit, so a scrollbar shows");
        assert_eq!(bar.height, 6);
        assert_eq!(bar.x, first.inner.right() - 1);
        // Without focusable rows the arrows scroll the text.
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            key_reaction(&model, &mut state, screen, down, Instant::now()),
            Reaction::Stay
        );
        assert_eq!(geometry(&model, &state, screen).start, 1);
        // PageDown moves a page less one row; End reaches the last page and stays inside it.
        let page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
        key_reaction(&model, &mut state, screen, page_down, Instant::now());
        assert_eq!(geometry(&model, &state, screen).start, 6);
        let end = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        key_reaction(&model, &mut state, screen, end, Instant::now());
        let last = geometry(&model, &state, screen);
        assert_eq!((last.start, last.end), (24, 30));
        // The wheel scrolls three rows per notch, only over the popup.
        let inside = Position::new(last.inner.x + 2, last.inner.y);
        wheel_reaction(&model, &mut state, screen, inside, -1);
        assert_eq!(geometry(&model, &state, screen).start, 21);
        wheel_reaction(&model, &mut state, screen, Position::new(0, 0), -1);
        assert_eq!(geometry(&model, &state, screen).start, 21);
        // A click at the top of the scrollbar goes back to the first rows.
        let top = Position::new(bar.x, bar.y);
        assert_eq!(
            click_reaction(&model, &mut state, screen, top, Instant::now()),
            Reaction::Stay
        );
        assert_eq!(geometry(&model, &state, screen).start, 0);
        // Escape still closes a scrolled popup.
        assert_eq!(react(&model, &mut state, KeyCode::Esc), Reaction::Dismiss);
    }

    #[test]
    fn moving_the_cursor_brings_a_scrolled_away_row_back_into_view() {
        let mut tree = briefing(30);
        let list = CheckListSpec {
            items: ["only"]
                .iter()
                .map(|id| CheckItem {
                    id: (*id).to_owned(),
                    label: span(id),
                    state: TriState::Off,
                    disabled_reason: None,
                    children: Vec::new(),
                })
                .collect(),
            ..CheckListSpec::default()
        };
        tree.nodes[0].children = vec![1];
        tree.nodes
            .push(node(Some("picks"), Widget::CheckList(list), Vec::new()));
        let model = model(&tree).expect("dialog");
        let mut state = state();
        let screen = Rect::new(0, 0, 80, 10);
        // The only check row is the last row; the cursor keeps it in view.
        assert_eq!(geometry(&model, &state, screen).end, 31);
        let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
        key_reaction(&model, &mut state, screen, page_up, Instant::now());
        assert!(geometry(&model, &state, screen).end < 31);
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        key_reaction(&model, &mut state, screen, up, Instant::now());
        assert_eq!(state.scroll, None);
        assert_eq!(geometry(&model, &state, screen).end, 31);
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sent(reaction: Reaction) -> Vec<UiAction> {
        match reaction {
            Reaction::Send(actions) => actions,
            other => panic!("expected actions, got {other:?}"),
        }
    }

    fn single(reaction: Reaction) -> UiAction {
        let mut actions = sent(reaction);
        assert_eq!(actions.len(), 1, "{actions:?}");
        actions.remove(0)
    }

    fn react(model: &PopupModel, state: &mut PopupState, code: KeyCode) -> Reaction {
        key_reaction(model, state, SCREEN, press(code), Instant::now())
    }

    #[test]
    fn a_dialog_view_becomes_message_rows_then_check_rows() {
        let model = model(&picker()).expect("the root is a dialog");
        assert_eq!(model.title, "Add panes to split");
        assert_eq!(model.rows.len(), 3);
        assert!(matches!(model.rows[0], Row::Text(_)));
        assert!(matches!(model.rows[2], Row::Check { .. }));
        assert_eq!(model.width, 72);
    }

    #[test]
    fn a_view_whose_root_is_not_a_dialog_is_refused() {
        let mut tree = picker();
        tree.root = 1;
        assert!(model(&tree).is_none());
    }

    #[test]
    fn space_toggles_the_check_item_under_the_cursor() {
        let model = model(&picker()).expect("dialog");
        let mut state = state();
        assert_eq!(react(&model, &mut state, KeyCode::Down), Reaction::Stay);
        let toggled = single(react(&model, &mut state, KeyCode::Char(' ')));
        assert_eq!(toggled.key, "members");
        assert_eq!(toggled.kind, ActionKind::Toggled);
        assert_eq!(
            toggled.value,
            Some(ActionValue::ItemState(("2".to_owned(), TriState::On)))
        );
    }

    #[test]
    fn enter_activates_the_primary_button_and_tab_moves_to_the_next() {
        let model = model(&picker()).expect("dialog");
        let mut state = state();
        let activated = single(react(&model, &mut state, KeyCode::Enter));
        assert_eq!(activated.action_id, "create");
        assert_eq!(activated.key, "dialog");
        assert_eq!(activated.kind, ActionKind::Activated);
        react(&model, &mut state, KeyCode::Tab);
        let activated = single(react(&model, &mut state, KeyCode::Enter));
        assert_eq!(activated.action_id, "cancel");
    }

    #[test]
    fn the_cursor_returns_to_the_top_when_the_plugin_shows_another_page() {
        let model = model(&picker()).expect("dialog");
        let mut state = state();
        state.page = Some("orientation".to_owned());
        state.row = 1;
        state.button = Some(0);
        sync(&model, &mut state);
        assert_eq!((state.row, state.button), (0, None));
        assert_eq!(state.page.as_deref(), Some("members"));

        state.row = 9;
        sync(&model, &mut state);
        assert_eq!(state.row, 1, "the cursor never passes the last row");
    }

    #[test]
    fn escape_always_dismisses() {
        let model = model(&picker()).expect("dialog");
        assert_eq!(react(&model, &mut state(), KeyCode::Esc), Reaction::Dismiss);
    }

    #[test]
    fn a_click_outside_dismisses_and_a_click_on_a_row_toggles_it() {
        let model = model(&picker()).expect("dialog");
        let screen = Rect::new(0, 0, 120, 40);
        let mut state = state();
        assert_eq!(
            click_reaction(
                &model,
                &mut state,
                screen,
                Position::new(0, 0),
                Instant::now()
            ),
            Reaction::Dismiss
        );
        let geometry = geometry(&model, &state, screen);
        let second_item = Position::new(geometry.inner.x + 2, geometry.inner.y + 2);
        let toggled = single(click_reaction(
            &model,
            &mut state,
            screen,
            second_item,
            Instant::now(),
        ));
        assert_eq!(state.row, 1);
        assert_eq!(
            toggled.value,
            Some(ActionValue::ItemState(("2".to_owned(), TriState::On)))
        );
    }

    #[test]
    fn a_click_on_a_button_activates_it() {
        let model = model(&picker()).expect("dialog");
        let screen = Rect::new(0, 0, 120, 40);
        let mut state = state();
        let geometry = geometry(&model, &state, screen);
        let cancel = geometry.buttons[0];
        let activated = single(click_reaction(
            &model,
            &mut state,
            screen,
            Position::new(cancel.x, cancel.y),
            Instant::now(),
        ));
        assert_eq!(activated.action_id, "cancel");
    }

    #[test]
    fn the_popup_never_exceeds_a_short_screen() {
        let model = model(&picker()).expect("dialog");
        let screen = Rect::new(0, 0, 30, 5);
        let geometry = geometry(&model, &state(), screen);
        assert!(geometry.popup.height <= 5);
        assert!(geometry.end - geometry.start <= 1);
    }

    #[test]
    fn text_input_and_number_widgets_become_editable_rows() {
        let model = model(&rename(0, false)).expect("dialog");
        assert!(matches!(&model.rows[0], Row::Input { key, .. } if key == "name"));
        assert!(matches!(&model.rows[1], Row::Number { key, .. } if key == "columns"));
        assert_eq!(focusable_rows(&model), vec![0, 1]);
    }

    #[test]
    fn typing_in_a_text_input_edits_it_and_reports_each_change() {
        let model = model(&rename(0, false)).expect("dialog");
        let mut state = state();
        let changed = single(react(&model, &mut state, KeyCode::Char('c')));
        assert_eq!(changed.key, "name");
        assert_eq!(changed.kind, ActionKind::Changed);
        assert_eq!(changed.value, Some(ActionValue::Text("abc".to_owned())));
        // Letters the dialog uses to move (j, k, Space) are text while an input has the cursor.
        let changed = single(react(&model, &mut state, KeyCode::Char('j')));
        assert_eq!(changed.value, Some(ActionValue::Text("abcj".to_owned())));
        assert_eq!(state.row, 0);
        let erased = single(react(&model, &mut state, KeyCode::Backspace));
        assert_eq!(erased.value, Some(ActionValue::Text("abc".to_owned())));
        // Control keys of the shared editor work too: Ctrl+U clears before the caret.
        let cleared = single(key_reaction(
            &model,
            &mut state,
            SCREEN,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Instant::now(),
        ));
        assert_eq!(cleared.value, Some(ActionValue::Text(String::new())));
    }

    #[test]
    fn enter_on_a_debounced_input_sends_its_value_before_the_button() {
        let model = model(&rename(500, false)).expect("dialog");
        let mut state = state();
        let now = Instant::now();
        assert_eq!(
            key_reaction(&model, &mut state, SCREEN, press(KeyCode::Char('x')), now),
            Reaction::Stay,
            "the change waits for its debounce"
        );
        let actions = sent(key_reaction(
            &model,
            &mut state,
            SCREEN,
            press(KeyCode::Enter),
            now,
        ));
        assert_eq!(actions.len(), 2, "{actions:?}");
        assert_eq!(actions[0].kind, ActionKind::Changed);
        assert_eq!(actions[0].value, Some(ActionValue::Text("abx".to_owned())));
        assert_eq!(actions[1].action_id, "create");
    }

    #[test]
    fn submit_on_enter_sends_submitted_with_the_text() {
        let model = model(&rename(0, true)).expect("dialog");
        let mut state = state();
        let submitted = single(react(&model, &mut state, KeyCode::Enter));
        assert_eq!(submitted.kind, ActionKind::Submitted);
        assert_eq!(submitted.value, Some(ActionValue::Text("ab".to_owned())));
    }

    #[test]
    fn a_debounced_edit_is_released_by_the_tick_and_by_leaving_the_row() {
        let model = model(&rename(200, false)).expect("dialog");
        let mut state = state();
        let start = Instant::now();
        key_reaction(&model, &mut state, SCREEN, press(KeyCode::Char('x')), start);
        assert!(state.edits.next_due().is_some());
        let mut released = Vec::new();
        release_due(
            &mut state,
            start + Duration::from_millis(250),
            &mut released,
        );
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].value, Some(ActionValue::Text("abx".to_owned())));

        key_reaction(&model, &mut state, SCREEN, press(KeyCode::Char('y')), start);
        let moved = single(key_reaction(
            &model,
            &mut state,
            SCREEN,
            press(KeyCode::Down),
            start,
        ));
        assert_eq!(moved.value, Some(ActionValue::Text("abxy".to_owned())));
        assert_eq!(state.row, 1);
    }

    #[test]
    fn a_number_row_steps_with_the_arrows_and_takes_typed_digits() {
        let model = model(&rename(0, false)).expect("dialog");
        let mut state = state();
        react(&model, &mut state, KeyCode::Down);
        let stepped = single(react(&model, &mut state, KeyCode::Right));
        assert_eq!(stepped.key, "columns");
        assert_eq!(stepped.value, Some(ActionValue::Number(3.0)));
        // A typed entry: digits open it, Enter commits it, clamped to the range.
        assert_eq!(
            react(&model, &mut state, KeyCode::Char('9')),
            Reaction::Stay
        );
        let typed = single(react(&model, &mut state, KeyCode::Enter));
        assert_eq!(typed.value, Some(ActionValue::Number(4.0)));
        // With no entry open, Enter belongs to the dialog's primary button.
        let activated = single(react(&model, &mut state, KeyCode::Enter));
        assert_eq!(activated.action_id, "create");
    }

    #[test]
    fn a_paste_lands_in_the_focused_input_only() {
        let model = model(&rename(0, false)).expect("dialog");
        let mut state = state();
        let pasted = single(paste_reaction(&model, &mut state, "cd", Instant::now()));
        assert_eq!(pasted.value, Some(ActionValue::Text("abcd".to_owned())));
        react(&model, &mut state, KeyCode::Down);
        assert_eq!(
            paste_reaction(&model, &mut state, "zz", Instant::now()),
            Reaction::Stay
        );
    }

    #[test]
    fn a_click_on_the_input_field_places_the_caret_and_the_stepper_arrows_step() {
        let model = model(&rename(0, false)).expect("dialog");
        let screen = Rect::new(0, 0, 120, 40);
        let mut state = state();
        let geometry = geometry(&model, &state, screen);
        let (_, field) = input_split(geometry.row_area(0).expect("shown"));
        let first_cell = Position::new(field.x, field.y);
        assert_eq!(
            click_reaction(&model, &mut state, screen, first_cell, Instant::now()),
            Reaction::Stay
        );
        let inserted = single(react(&model, &mut state, KeyCode::Char('z')));
        assert_eq!(inserted.value, Some(ActionValue::Text("zab".to_owned())));

        let area = geometry.row_area(1).expect("shown");
        let edit = NumberEdit::seeded(match &model.rows[1] {
            Row::Number { spec, .. } => spec,
            other => panic!("a number row, got {other:?}"),
        });
        let Row::Number { spec, .. } = &model.rows[1] else {
            unreachable!("checked above");
        };
        let stepper = number_field(area, &number_text(spec, &edit));
        let up = single(click_reaction(
            &model,
            &mut state,
            screen,
            Position::new(stepper.right() - 1, stepper.y),
            Instant::now(),
        ));
        assert_eq!(state.row, 1);
        assert_eq!(up.value, Some(ActionValue::Number(3.0)));
    }

    #[test]
    fn the_input_field_and_its_label_never_overlap() {
        let area = Rect::new(4, 3, 40, 1);
        let (label_room, field) = input_split(area);
        assert_eq!(area.x + label_room, field.x);
        assert_eq!(field.right(), area.right());
        assert!(field.width >= MIN_FIELD_CELLS);
        let narrow = Rect::new(0, 0, 6, 1);
        let (label_room, field) = input_split(narrow);
        assert_eq!(label_room, 0);
        assert_eq!(field.width, 6);
    }

    #[test]
    fn closing_a_popup_returns_to_the_settings_page_that_asked_for_it() {
        let mut app = App::new("popup over settings".to_owned(), std::env::temp_dir());
        // A popup that nothing asked for from a settings page closes to the normal view.
        app.mode = Mode::PluginPopup(PopupState::new("p".to_owned(), "q".to_owned()));
        app.close_plugin_popup_mode();
        assert!(matches!(app.mode, Mode::Normal));
        // A popup opened over Settings brings the page back where it was.
        let settings = crate::app::SettingsState {
            selected_row: 7,
            ..crate::app::SettingsState::default()
        };
        app.settings_suspended_for_popup = Some(Box::new(settings));
        app.mode = Mode::PluginPopup(PopupState::new("p".to_owned(), "q".to_owned()));
        app.close_plugin_popup_mode();
        let Mode::Settings(back) = &app.mode else {
            panic!("the settings page returns");
        };
        assert_eq!(back.selected_row, 7);
        assert!(app.settings_suspended_for_popup.is_none());
    }
}
