//! Live editor for the tree toolbar and its inactivity threshold.
use crate::{
    app::{App, SettingsState},
    tree_ui::TreeToolbarAction,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

fn change(app: &mut App, state: &mut SettingsState, key: KeyEvent) -> bool {
    let mut ui = app.ui_settings.clone();
    let len = ui.tree_toolbar_layout.entries.len();
    if let Some(selected) = state.toolbar_picker {
        let available: Vec<_> = ui.tree_toolbar_layout.available().collect();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => state.toolbar_picker = None,
            KeyCode::Up => state.toolbar_picker = Some(selected.saturating_sub(1)),
            KeyCode::Down => {
                state.toolbar_picker = Some((selected + 1).min(available.len().saturating_sub(1)))
            }
            KeyCode::Enter => {
                if let Some(entry) = available.get(selected) {
                    ui.tree_toolbar_layout.entries.push(*entry);
                    state.selected_row = ui.tree_toolbar_layout.entries.len() - 1;
                    app.apply_and_persist_ui_settings(ui);
                }
                state.toolbar_picker = None;
            }
            _ => {}
        }
        return true;
    }
    state.selected_row = state.selected_row.min(len + 1);
    match key.code {
        KeyCode::Up | KeyCode::Down => {
            let next = if key.code == KeyCode::Up {
                state.selected_row.saturating_sub(1)
            } else {
                (state.selected_row + 1).min(len + 1)
            };
            if key.modifiers.contains(KeyModifiers::SHIFT) && state.selected_row < len && next < len
            {
                ui.tree_toolbar_layout.move_entry(state.selected_row, next);
                app.apply_and_persist_ui_settings(ui);
            }
            state.selected_row = next;
        }
        KeyCode::Delete | KeyCode::Backspace if state.selected_row < len => {
            let removed = ui.tree_toolbar_layout.entries.remove(state.selected_row);
            if removed == crate::tree_toolbar_layout::TreeToolbarEntry::ActivityFilter
                && app.hide_inactive_tree
            {
                app.hide_inactive_tree = false;
                app.tick_tree_activity_projection(std::time::Instant::now());
            }
            state.selected_row = state.selected_row.min(ui.tree_toolbar_layout.entries.len());
            app.apply_and_persist_ui_settings(ui);
        }
        KeyCode::Char('+') => state.toolbar_picker = Some(0),
        KeyCode::Enter if state.selected_row == len => state.toolbar_picker = Some(0),
        KeyCode::Left | KeyCode::Right if state.selected_row == len + 1 => {
            ui.tree_inactivity_minutes = if key.code == KeyCode::Left {
                ui.tree_inactivity_minutes.saturating_sub(1).max(1)
            } else {
                ui.tree_inactivity_minutes.saturating_add(1).min(1440)
            };
            app.apply_and_persist_ui_settings(ui);
        }
        KeyCode::Char('r') => {
            ui.tree_toolbar_layout = Default::default();
            app.apply_and_persist_ui_settings(ui);
        }
        _ => return false,
    }
    true
}

pub fn handle_key(app: &mut App, state: &mut SettingsState, key: KeyEvent) -> bool {
    change(app, state, key)
}

fn columns(area: Rect) -> (Rect, Rect) {
    if area.width >= 70 {
        let width = area.width / 2;
        (
            Rect::new(area.x, area.y, width, area.height),
            Rect::new(
                area.x + width + 1,
                area.y,
                area.width - width - 1,
                area.height,
            ),
        )
    } else {
        (
            Rect::new(area.x, area.y, area.width, area.height.saturating_sub(3)),
            Rect::new(
                area.x,
                area.y.saturating_add(area.height.saturating_sub(3)),
                area.width,
                area.height.min(3),
            ),
        )
    }
}

fn first_visible(state: &SettingsState, height: usize) -> usize {
    state
        .selected_row
        .saturating_add(1)
        .saturating_sub(height.max(1))
}

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, state: &SettingsState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let entries = &app.ui_settings.tree_toolbar_layout.entries;
    let (list, preview_area) = columns(area);
    let mut lines = vec![
        Line::from("Tree toolbar"),
        Line::from("↑↓ select  Shift+↑↓ move  Del remove  + add  r reset"),
    ];
    let available_height = usize::from(list.height.saturating_sub(2));
    let first = first_visible(state, available_height);
    for row in first..(entries.len() + 2).min(first + available_height) {
        let text = if row < entries.len() {
            let action = TreeToolbarAction::from(entries[row]);
            format!(
                "{}  {}",
                action.glyph(&app.ui_settings.icons),
                action.description()
            )
        } else if row == entries.len() {
            "+  Add button".to_owned()
        } else {
            format!(
                "Inactive after  ‹ {} minutes ›",
                app.ui_settings.tree_inactivity_minutes
            )
        };
        let style = if row == state.selected_row {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }
    frame.render_widget(Paragraph::new(lines), list);
    if list.width >= 12 {
        for row in first..entries.len().min(first + available_height) {
            let y = list.y + 2 + (row - first) as u16;
            frame.render_widget(
                Paragraph::new("↑  ↓  ×"),
                Rect::new(list.right() - 7, y, 7, 1),
            );
        }
    }
    frame.render_widget(
        Paragraph::new("Live toolbar preview"),
        Rect::new(preview_area.x, preview_area.y, preview_area.width, 1),
    );
    let actual_width = crate::tree_ui::toolbar_area(app.layout.tree_area).width;
    let preview_width = if actual_width == 0 {
        preview_area.width
    } else {
        preview_area.width.min(actual_width)
    };
    let preview = Rect::new(
        preview_area.x,
        preview_area.y.saturating_add(1),
        preview_width,
        u16::from(preview_area.height > 1),
    );
    // Reuse the same button geometry as the tree, whose toolbar is on the last row.
    for (action, rect) in crate::tree_ui::configured_toolbar_rects_in_strip(
        preview,
        &app.ui_settings.tree_toolbar_layout,
    ) {
        let style = if app.hide_inactive_tree && action == TreeToolbarAction::ActivityFilter {
            Style::new()
                .fg(crate::theme::accent_fg())
                .bg(action.accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(action.accent())
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", action.glyph(&app.ui_settings.icons)),
                style,
            ))),
            rect,
        );
    }
    if let Some(selected) = state.toolbar_picker {
        let available: Vec<_> = app.ui_settings.tree_toolbar_layout.available().collect();
        let width = area.width.min(48);
        let height = area.height.min((available.len() as u16).saturating_add(3));
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        let mut choices = vec![Line::from("Add button — Enter to add, Esc to cancel")];
        if available.is_empty() {
            choices.push(Line::from("All available buttons are already present."));
        }
        let start = selected
            .saturating_add(1)
            .saturating_sub(usize::from(height.saturating_sub(1)).max(1));
        for (index, entry) in available
            .iter()
            .enumerate()
            .skip(start)
            .take(usize::from(height.saturating_sub(1)))
        {
            let action = TreeToolbarAction::from(*entry);
            choices.push(Line::from(Span::styled(
                format!(
                    "{}  {}",
                    action.glyph(&app.ui_settings.icons),
                    action.description()
                ),
                if index == selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )));
        }
        frame.render_widget(Paragraph::new(choices), popup);
    }
}

pub fn handle_mouse(app: &mut App, state: &mut SettingsState, area: Rect, mouse: MouseEvent) {
    if matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        let code = if mouse.kind == MouseEventKind::ScrollUp {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        change(app, state, KeyEvent::new(code, KeyModifiers::NONE));
        return;
    }
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    if let Some(selected) = state.toolbar_picker {
        let available: Vec<_> = app.ui_settings.tree_toolbar_layout.available().collect();
        let width = area.width.min(48);
        let height = area.height.min((available.len() as u16).saturating_add(3));
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        if !popup.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
            state.toolbar_picker = None;
            return;
        }
        let start = selected
            .saturating_add(1)
            .saturating_sub(usize::from(height.saturating_sub(1)).max(1));
        let index = start + usize::from(mouse.row.saturating_sub(popup.y + 1));
        if mouse.row > popup.y && index < available.len() {
            state.toolbar_picker = Some(index);
            change(
                app,
                state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            );
        }
        return;
    }
    let (list, _) = columns(area);
    if !list.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
        return;
    }
    let height = usize::from(list.height.saturating_sub(2));
    let row = usize::from(mouse.row.saturating_sub(list.y));
    if row >= 2 && row < 2 + height {
        let index = first_visible(state, height) + row - 2;
        let len = app.ui_settings.tree_toolbar_layout.entries.len();
        if index <= len + 1 {
            state.selected_row = index;
            if index < len && list.width >= 12 && mouse.column >= list.right() - 7 {
                let offset = mouse.column - (list.right() - 7);
                let (code, modifiers) = if offset < 2 {
                    (KeyCode::Up, KeyModifiers::SHIFT)
                } else if offset < 5 {
                    (KeyCode::Down, KeyModifiers::SHIFT)
                } else {
                    (KeyCode::Delete, KeyModifiers::NONE)
                };
                change(app, state, KeyEvent::new(code, modifiers));
            } else if index == len {
                state.toolbar_picker = Some(0);
            } else if index == len + 1 {
                let code = if mouse.column < list.x + list.width / 2 {
                    KeyCode::Left
                } else {
                    KeyCode::Right
                };
                change(app, state, KeyEvent::new(code, KeyModifiers::NONE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_toolbar_layout::{TreeToolbarEntry, TreeToolbarLayout};

    #[test]
    fn keyboard_removes_readds_and_reorders_buttons() {
        let mut app = App::new("toolbar settings test".to_owned(), std::env::temp_dir());
        let mut state = SettingsState::default();
        let original = app.ui_settings.tree_toolbar_layout.entries[0];
        assert!(handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)
        ));
        assert!(!app
            .ui_settings
            .tree_toolbar_layout
            .entries
            .contains(&original));
        assert!(handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE)
        ));
        assert!(state.toolbar_picker.is_some());
        assert!(handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
        assert_eq!(
            app.ui_settings.tree_toolbar_layout.entries.last(),
            Some(&original)
        );
        assert!(handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)
        ));
        let moved = state.selected_row;
        assert_eq!(app.ui_settings.tree_toolbar_layout.entries[moved], original);
    }

    #[test]
    fn wide_settings_show_preview_next_to_list_and_narrow_settings_stack() {
        let (list, preview) = columns(Rect::new(0, 0, 100, 24));
        assert!(preview.x >= list.right());
        let (list, preview) = columns(Rect::new(0, 0, 50, 24));
        assert!(preview.y >= list.bottom());
    }

    #[test]
    fn default_toolbar_keeps_eye_and_settings_in_a_narrow_strip() {
        let strip = Rect::new(0, 0, 20, 1);
        let buttons =
            crate::tree_ui::configured_toolbar_rects_in_strip(strip, &TreeToolbarLayout::default());
        assert!(buttons
            .iter()
            .any(|(action, _)| *action == TreeToolbarAction::ActivityFilter));
        assert!(buttons
            .iter()
            .any(|(action, _)| *action == TreeToolbarAction::Settings));
        for pair in buttons.windows(2) {
            assert!(pair[0].1.right() <= pair[1].1.x);
        }
    }

    #[test]
    fn preview_uses_the_same_positions_as_the_tree_toolbar() {
        let layout = TreeToolbarLayout {
            entries: vec![TreeToolbarEntry::Settings, TreeToolbarEntry::ActivityFilter],
        };
        let strip = Rect::new(3, 4, 20, 1);
        let buttons = crate::tree_ui::configured_toolbar_rects_in_strip(strip, &layout);
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].0, TreeToolbarAction::Settings);
        assert_eq!(buttons[1].0, TreeToolbarAction::ActivityFilter);
        assert!(buttons[0].1.right() <= buttons[1].1.x);
    }
}
