//! Keyboard editor for the shared tree row geometry.
use crate::{
    app::{App, SettingsState},
    icon_settings::IconTarget,
    theme,
    tree_row_layout::{TreeRowElement, TreeRowLayout},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
#[derive(Debug, Clone, Default)]
pub struct EditorState {
    pub selected: usize,
    pub moving: bool,
    original: Option<TreeRowLayout>,
}
impl EditorState {
    /// Returns whether persisted layout data changed. Navigation never saves.
    fn apply(&mut self, layout: &mut TreeRowLayout, key: KeyEvent) -> bool {
        let before = layout.clone();
        match key.code {
            KeyCode::Enter => self.moving = !self.moving,
            KeyCode::Up | KeyCode::Down => {
                let next = if key.code == KeyCode::Up {
                    self.selected.saturating_sub(1)
                } else {
                    (self.selected + 1).min(3)
                };
                if self.moving || key.modifiers.contains(KeyModifiers::SHIFT) {
                    layout.order.swap(self.selected, next);
                }
                self.selected = next;
            }
            KeyCode::Char(' ') => {
                self.moving = false;
                let element = layout.order[self.selected];
                if element != TreeRowElement::Title {
                    if layout.hidden.contains(&element) {
                        layout.hidden.retain(|item| *item != element);
                    } else {
                        layout.hidden.push(element);
                    }
                }
            }
            KeyCode::Char('1' | 'r' | 'R') => {
                self.moving = false;
                *layout = TreeRowLayout::default();
            }
            KeyCode::Char('2') => {
                self.moving = false;
                *layout = TreeRowLayout::all_right();
            }
            KeyCode::Char('3') => {
                self.moving = false;
                *layout = TreeRowLayout::mixed();
            }
            _ => {}
        }
        *layout = layout.clone().normalized();
        before != *layout
    }
}
pub fn handle_key(app: &mut App, state: &mut SettingsState, key: KeyEvent) -> bool {
    let Some(editor) = state.tree_layout_editor.as_mut() else {
        return false;
    };
    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
        if let Some(original) = editor.original.take() {
            app.ui_settings.tree_row_layout = original;
            editor.moving = false;
        } else {
            state.tree_layout_editor = None;
        }
        return true;
    }
    let mut layout = app.ui_settings.tree_row_layout.clone();
    if key.code == KeyCode::Enter && !editor.moving {
        editor.original = Some(layout.clone());
    }
    let changed = editor.apply(&mut layout, key);
    if editor.moving {
        if changed {
            app.ui_settings.tree_row_layout = layout;
        }
    } else {
        let finished_move = editor.original.take().is_some();
        if changed || finished_move {
            app.settings_set_tree_row_layout(layout);
        }
    }
    true
}
/// Leaving through the surrounding settings chrome cancels an uncommitted move.
pub fn dismiss(app: &mut App, state: &mut SettingsState) {
    if let Some(editor) = state.tree_layout_editor.take() {
        if let Some(original) = editor.original {
            app.ui_settings.tree_row_layout = original;
        }
    }
}
/// The settings body is substantially narrower than the terminal itself.
/// Stack until both panes can show complete controls and a useful specimen.
pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &EditorState) {
    let is_stacked = area.width < 86;
    let constraints = if is_stacked {
        [Constraint::Length(11), Constraint::Min(0)]
    } else {
        [Constraint::Length(46), Constraint::Min(0)]
    };
    let columns = Layout::default()
        .direction(if is_stacked {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints(constraints)
        .split(area);
    let layout = &app.ui_settings.tree_row_layout;
    let mut lines = vec![Line::from("Above Title: left · below: right-aligned")];
    let mut right = false;
    for (index, &element) in layout.order().iter().enumerate() {
        let side = if element == TreeRowElement::Title {
            "divider"
        } else if right {
            "right"
        } else {
            "left"
        };
        let visible = if element == TreeRowElement::Title {
            "required"
        } else if layout.is_visible(element) {
            "shown"
        } else {
            "hidden"
        };
        let marker = if index == state.selected {
            if state.moving {
                "↕"
            } else {
                "›"
            }
        } else {
            " "
        };
        let label = if element == TreeRowElement::Title {
            "Title"
        } else {
            element.label()
        };
        let text = format!("{marker} {label} · {side} · {visible}");
        lines.push(Line::from(Span::styled(
            text,
            if index == state.selected {
                theme::selected_style()
            } else {
                Default::default()
            },
        )));
        if element == TreeRowElement::Title {
            right = true;
        }
    }
    lines.extend([
        Line::from("↑↓ select · Enter move/save · Esc cancel/back"),
        Line::from("Shift+↑↓ move · Space show/hide"),
        Line::from("1 Default · 2 All right · 3 Mixed · R reset"),
        Line::from("Badges follow Title; cost/actions stay fixed"),
    ]);
    frame.render_widget(
        Paragraph::new(lines).block(theme::block(true).title("Tree layout")),
        columns[0],
    );

    let width = usize::from(columns[1].width.saturating_sub(2));
    let icons = &app.ui_settings.icons;
    let row = |trunk: &'static str,
               identity,
               objective: Option<IconTarget>,
               activity: Option<IconTarget>,
               title: &str| {
        let mut line = layout.compose(
            preview_slot(icons.glyph(identity), 3),
            objective.map_or_else(
                || Span::raw(if layout.is_default() { "   " } else { "" }),
                |target| {
                    preview_slot(
                        icons.glyph(target),
                        crate::status_icons::OBJECTIVE_COLUMN_WIDTH,
                    )
                },
            ),
            activity.map_or_else(
                || Span::raw(if layout.is_default() { "  " } else { "" }),
                |target| preview_slot(icons.glyph(target), crate::status_icons::NOW_COLUMN_WIDTH),
            ),
            Span::raw(title.to_owned()),
            width.saturating_sub(unicode_width::UnicodeWidthStr::width(trunk)),
        );
        line.spans.insert(0, Span::raw(trunk));
        line
    };
    let preview = vec![
        row("", IconTarget::Group, None, None, "Workspace"),
        row("└─ ", IconTarget::SplitVertical, None, None, "Build"),
        row(
            "   ├─ ",
            IconTarget::Claude,
            Some(IconTarget::GoalActive),
            Some(IconTarget::Working),
            "Working on settings",
        ),
        row(
            "   ├─ ",
            IconTarget::Codex,
            Some(IconTarget::GoalBlocked),
            Some(IconTarget::WaitingApproval),
            "Waiting for approval",
        ),
        row(
            "   └─ ",
            IconTarget::OtherAgent,
            Some(IconTarget::TaskDone),
            Some(IconTarget::Done),
            "Finished task",
        ),
    ];
    frame.render_widget(
        Paragraph::new(preview).block(theme::block(false).title("Preview · dummy tree")),
        columns[1],
    );
}

/// Match the real normal-mode slot widths, including wide/custom glyphs.
fn preview_slot(glyph: &str, width: usize) -> Span<'static> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let mut text = String::new();
    let mut used = 0;
    for grapheme in glyph.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells > width {
            break;
        }
        text.push_str(grapheme);
        used += cells;
    }
    text.push_str(&" ".repeat(width.saturating_sub(used)));
    Span::raw(text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_move_restores_layout_and_keeps_editor_open() {
        let mut app = App::new("tree layout cancellation".to_owned(), std::env::temp_dir());
        let original = app.ui_settings.tree_row_layout.clone();
        let mut state = SettingsState {
            tree_layout_editor: Some(EditorState::default()),
            ..SettingsState::default()
        };
        handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_ne!(app.ui_settings.tree_row_layout, original);
        handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(app.ui_settings.tree_row_layout, original);
        assert!(state.tree_layout_editor.is_some());
        handle_key(
            &mut app,
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(state.tree_layout_editor.is_none());
    }

    #[test]
    fn keyboard_moves_title_across_icons_and_protects_visibility() {
        let mut editor = EditorState::default();
        let mut layout = TreeRowLayout::default();
        assert!(!editor.apply(
            &mut layout,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
        assert!(editor.apply(
            &mut layout,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
        ));
        assert_eq!(layout.order()[1], TreeRowElement::Identity);
        editor.selected = 3;
        assert!(!editor.apply(
            &mut layout,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
        ));
        assert!(layout.is_visible(TreeRowElement::Title));
        editor.apply(
            &mut layout,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        );
        assert_eq!(layout.order()[0], TreeRowElement::Title);
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    #[test]
    fn editor_keeps_every_element_and_preview_visible_in_actual_settings_bodies() {
        for (width, height) in [(49, 19), (69, 19), (100, 24)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let mut app = App::new("tree layout test".to_owned(), std::env::temp_dir());
            app.ui_settings
                .tree_row_layout
                .hidden
                .push(TreeRowElement::Activity);
            let state = EditorState {
                selected: 3,
                moving: true,
                ..Default::default()
            };
            terminal
                .draw(|frame| render(frame, frame.area(), &app, &state))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            for label in [
                "Identity",
                "Objective",
                "Activity",
                "↕ Title",
                "required",
                "hidden",
                "Enter move/save",
                "Preview",
                "Workspace",
                "Working",
                "Waiting",
                "Finished",
            ] {
                assert!(text.contains(label), "missing {label} at {width}×{height}");
            }
        }
    }

    #[test]
    fn preview_slots_match_normal_live_widths_without_splitting_glyphs() {
        assert_eq!(preview_slot("🦀", 3).width(), 3);
        assert_eq!(preview_slot("🎯", 3).width(), 3);
        assert_eq!(preview_slot("⠋", 2).width(), 2);
        assert_eq!(preview_slot("👩‍💻", 1).content.as_ref(), " ");
    }
}
