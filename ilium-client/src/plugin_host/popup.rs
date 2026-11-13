//! The kernel's popup host (foundation contract section 15.4).
//!
//! A client-side plugin opens a popup with the `open-view` intent and describes it with a view
//! whose root is a `dialog` widget. This module draws that view with the kernel's own modal
//! chrome and turns keys and clicks into `ui-action` events for the plugin. It understands the
//! widgets a dialog needs (message lines, `paragraph`, `choice` drawn as radio rows,
//! `check-list`); the plugin keeps every value, the kernel keeps only the cursor. Escape and a
//! click outside always close the popup here, whatever the plugin does, so a stuck or trapped
//! plugin can never hold the screen.

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ilium_plugin_api::generated::types::{
    ActionKind, ActionValue, ButtonRole, Constraint, DialogButton, Span as PluginSpan, TriState,
    UiAction, UiTree, ViewTarget, Widget,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::{modal, theme};

/// Width of a popup whose dialog does not ask for a fixed one.
const DEFAULT_WIDTH: u16 = 64;
/// Rows of chrome around the content: two border rows, one blank row, one button row.
const CHROME_ROWS: u16 = 4;
/// The key the kernel reports dialog buttons under when the root node has no key.
const DIALOG_KEY: &str = "dialog";

/// What the kernel remembers about the open popup: whose it is and where the cursor sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupState {
    pub plugin: String,
    pub popup: String,
    /// Index into the focusable rows (choice options and check items).
    pub row: usize,
    /// Index of the focused button; `None` until the first draw picks the primary one.
    pub button: Option<usize>,
    /// Key of the widget the cursor was last placed in. When the plugin shows another page
    /// the key changes and the cursor starts again at the top.
    pub page: Option<String>,
}

impl PopupState {
    pub(crate) fn new(plugin: String, popup: String) -> Self {
        Self {
            plugin,
            popup,
            row: 0,
            button: None,
            page: None,
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
}

impl Row {
    fn is_focusable(&self) -> bool {
        !matches!(self, Row::Text(_))
    }

    fn key(&self) -> Option<&str> {
        match self {
            Row::Text(_) => None,
            Row::Choice { key, .. } | Row::Check { key, .. } => Some(key),
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
        let key = node.key.clone().unwrap_or_default();
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

/// Brings the cursor in line with the view the plugin now shows: back to the top (and to the
/// primary button) when the page changed, and never past the last row.
pub(crate) fn sync(model: &PopupModel, state: &mut PopupState) {
    let page = model.rows.iter().find_map(Row::key).map(str::to_owned);
    if state.page != page {
        state.page = page;
        state.row = 0;
        state.button = None;
    }
    state.row = state.row.min(focusable_rows(model).len().saturating_sub(1));
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
    let start = cursor
        .saturating_sub(capacity / 2)
        .min(model.rows.len().saturating_sub(capacity));
    let end = (start + capacity).min(model.rows.len());
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
    }
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

/// The action a row sends when it is chosen with Space, a click, or by moving onto a choice.
fn row_action(model: &PopupModel, state: &PopupState, pick: bool) -> Option<UiAction> {
    let index = *focusable_rows(model).get(state.row)?;
    match model.rows.get(index)? {
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
    /// Nothing for the plugin; the cursor may have moved.
    Stay,
    /// The plugin gets this action; the popup stays until the plugin closes it.
    Send(UiAction),
    /// The kernel closes the popup and tells the plugin.
    Dismiss,
}

pub(crate) fn key_reaction(model: &PopupModel, state: &mut PopupState, key: KeyCode) -> Reaction {
    sync(model, state);
    let focusable = focusable_rows(model).len();
    let buttons = model.buttons.len();
    let current_button = focused_button(model, state);
    match key {
        KeyCode::Esc => Reaction::Dismiss,
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
            let down = matches!(key, KeyCode::Down | KeyCode::Char('j'));
            state.row = if down {
                (state.row + 1).min(focusable.saturating_sub(1))
            } else {
                state.row.saturating_sub(1)
            };
            row_action(model, state, false).map_or(Reaction::Stay, Reaction::Send)
        }
        KeyCode::Char(' ') => row_action(model, state, true).map_or(Reaction::Stay, Reaction::Send),
        KeyCode::Tab | KeyCode::Right if buttons > 0 => {
            state.button = Some((current_button + 1) % buttons);
            Reaction::Stay
        }
        KeyCode::BackTab | KeyCode::Left if buttons > 0 => {
            state.button = Some((current_button + buttons - 1) % buttons);
            Reaction::Stay
        }
        KeyCode::Enter => {
            button_action(model, state, current_button).map_or(Reaction::Stay, Reaction::Send)
        }
        _ => Reaction::Stay,
    }
}

pub(crate) fn click_reaction(
    model: &PopupModel,
    state: &mut PopupState,
    screen: Rect,
    position: Position,
) -> Reaction {
    sync(model, state);
    let geometry = geometry(model, state, screen);
    if !geometry.popup.contains(position) {
        return Reaction::Dismiss;
    }
    if let Some(index) = geometry
        .buttons
        .iter()
        .position(|button| button.contains(position))
    {
        state.button = Some(index);
        return button_action(model, state, index).map_or(Reaction::Stay, Reaction::Send);
    }
    if !geometry.inner.contains(position) {
        return Reaction::Stay;
    }
    let row = geometry.start + usize::from(position.y.saturating_sub(geometry.inner.y));
    if row >= geometry.end {
        return Reaction::Stay;
    }
    let Some(focus) = focusable_rows(model).iter().position(|index| *index == row) else {
        return Reaction::Stay;
    };
    state.row = focus;
    row_action(model, state, true).map_or(Reaction::Stay, Reaction::Send)
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
    let lines: Vec<Line> = model.rows[geometry.start..geometry.end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let is_cursor = cursor == Some(geometry.start + offset);
            let marker = if is_cursor { "›" } else { " " };
            let style = if is_cursor {
                theme::selected_style()
            } else {
                Style::new()
            };
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
            }
        })
        .collect();
    let content = Rect::new(
        geometry.inner.x,
        geometry.inner.y,
        geometry.inner.width,
        geometry.inner.height.saturating_sub(2),
    );
    frame.render_widget(Paragraph::new(lines), content);
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
            Reaction::Send(action) => {
                if let Some(plugins) = self.wasm_plugins.as_ref() {
                    plugins.popup_action(&state.plugin, action, self.popup_screen());
                }
                self.mode = Mode::PluginPopup(state);
            }
            Reaction::Dismiss => {
                if let Some(plugins) = self.wasm_plugins.as_ref() {
                    plugins.dismiss_popup(&state.plugin, &state.popup);
                }
                self.mode = Mode::Normal;
            }
        }
    }

    pub(crate) fn popup_screen(&self) -> (u16, u16) {
        (
            self.layout.screen_area.width,
            self.layout.screen_area.height,
        )
    }

    /// One key press while a plugin popup is open. The caller took the state out of `mode`.
    pub(crate) fn plugin_popup_key(&mut self, mut state: PopupState, key: KeyCode) {
        let reaction = match self.plugin_popup_model(&state) {
            Some(model) => key_reaction(&model, &mut state, key),
            // No view yet: only Escape does anything.
            None if key == KeyCode::Esc => Reaction::Dismiss,
            None => Reaction::Stay,
        };
        self.apply_popup_reaction(state, reaction);
    }

    /// One mouse event while a plugin popup is open. The caller took the state out of `mode`.
    pub(crate) fn plugin_popup_mouse(&mut self, mut state: PopupState, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.mode = Mode::PluginPopup(state);
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let screen = self.layout.screen_area;
        let reaction = match self.plugin_popup_model(&state) {
            Some(model) => click_reaction(&model, &mut state, screen, position),
            None => Reaction::Dismiss,
        };
        self.apply_popup_reaction(state, reaction);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn picker() -> UiTree {
        let dialog = DialogSpec {
            title: span("Add panes to split"),
            kind: DialogKind::Plain,
            buttons: vec![
                button("cancel", ButtonRole::Secondary, true),
                button("create", ButtonRole::Primary, false),
            ],
            width: Constraint::Length(72),
            height: Constraint::Min(0),
            message: Some(vec![span("Select up to four panes (0/4).")]),
        };
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
                node(Some("dialog"), Widget::Dialog(dialog), vec![1]),
                node(Some("members"), Widget::CheckList(list), Vec::new()),
            ],
            root: 0,
        }
    }

    fn state() -> PopupState {
        PopupState::new("ilium.core.layout".to_owned(), "new-split".to_owned())
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
        assert_eq!(
            key_reaction(&model, &mut state, KeyCode::Down),
            Reaction::Stay
        );
        let Reaction::Send(sent) = key_reaction(&model, &mut state, KeyCode::Char(' ')) else {
            panic!("space sends a toggle");
        };
        assert_eq!(sent.key, "members");
        assert_eq!(sent.kind, ActionKind::Toggled);
        assert_eq!(
            sent.value,
            Some(ActionValue::ItemState(("2".to_owned(), TriState::On)))
        );
    }

    #[test]
    fn enter_activates_the_primary_button_and_tab_moves_to_the_next() {
        let model = model(&picker()).expect("dialog");
        let mut state = state();
        let Reaction::Send(sent) = key_reaction(&model, &mut state, KeyCode::Enter) else {
            panic!("enter activates a button");
        };
        assert_eq!(sent.action_id, "create");
        assert_eq!(sent.key, "dialog");
        assert_eq!(sent.kind, ActionKind::Activated);
        key_reaction(&model, &mut state, KeyCode::Tab);
        let Reaction::Send(sent) = key_reaction(&model, &mut state, KeyCode::Enter) else {
            panic!("enter activates a button");
        };
        assert_eq!(sent.action_id, "cancel");
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
        assert_eq!(
            key_reaction(&model, &mut state(), KeyCode::Esc),
            Reaction::Dismiss
        );
    }

    #[test]
    fn a_click_outside_dismisses_and_a_click_on_a_row_toggles_it() {
        let model = model(&picker()).expect("dialog");
        let screen = Rect::new(0, 0, 120, 40);
        let mut state = state();
        assert_eq!(
            click_reaction(&model, &mut state, screen, Position::new(0, 0)),
            Reaction::Dismiss
        );
        let geometry = geometry(&model, &state, screen);
        let second_item = Position::new(geometry.inner.x + 2, geometry.inner.y + 2);
        let Reaction::Send(sent) = click_reaction(&model, &mut state, screen, second_item) else {
            panic!("a click on a check row toggles it");
        };
        assert_eq!(state.row, 1);
        assert_eq!(
            sent.value,
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
        let Reaction::Send(sent) = click_reaction(
            &model,
            &mut state,
            screen,
            Position::new(cancel.x, cancel.y),
        ) else {
            panic!("a click on a button activates it");
        };
        assert_eq!(sent.action_id, "cancel");
    }

    #[test]
    fn the_popup_never_exceeds_a_short_screen() {
        let model = model(&picker()).expect("dialog");
        let screen = Rect::new(0, 0, 30, 5);
        let geometry = geometry(&model, &state(), screen);
        assert!(geometry.popup.height <= 5);
        assert!(geometry.end - geometry.start <= 1);
    }
}
