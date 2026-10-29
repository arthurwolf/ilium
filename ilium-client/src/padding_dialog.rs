//! The "Padding fine-tuning" dialog: settings on the left, a live neutral
//! preview (dummy tree beside a dummy agent) on the right.
//!
//! [`items`] is the single source of the dialog's clickable geometry; [`render`]
//! and [`hit`] both read it.

use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::padding::{PaddingPanel, PaddingSettings, MAX_PADDING};
use crate::padding_app::PaddingSide;
use crate::theme;
use crate::value_control::boolean_switch;

const SETTINGS_WIDTH: u16 = 46;
const BUTTON_WIDTH: u16 = 5;

/// Which right-hand panel type the second section edits (only when the types
/// do not share one setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogState {
    pub right_type: PaddingPanel,
    pub hover: Option<DialogHit>,
}

impl Default for DialogState {
    fn default() -> Self {
        Self {
            right_type: PaddingPanel::Terminal,
            hover: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogHit {
    Close,
    Linked(PaddingPanel),
    Shared(bool),
    RightType(PaddingPanel),
    /// `side == None` is the linked "all sides" value.
    Value {
        panel: PaddingPanel,
        side: Option<PaddingSide>,
        cells: u8,
    },
}

/// The panel a section edits.
fn section_panel(state: &DialogState, settings: &PaddingSettings, is_tree: bool) -> PaddingPanel {
    if is_tree {
        PaddingPanel::Tree
    } else if settings.right_panels_shared {
        PaddingPanel::Terminal
    } else {
        state.right_type
    }
}

pub fn dialog_area(screen: Rect) -> Rect {
    let margin_x = (screen.width / 16).max(1);
    let margin_y = (screen.height / 12).max(1);
    Rect::new(
        screen.x + margin_x,
        screen.y + margin_y,
        screen.width.saturating_sub(margin_x * 2),
        screen.height.saturating_sub(margin_y * 2),
    )
}

/// Settings column and preview column inside the dialog's inner area.
fn columns(inner: Rect) -> (Rect, Rect) {
    let settings_width = SETTINGS_WIDTH.min(inner.width / 2);
    let settings = Rect::new(inner.x, inner.y, settings_width, inner.height);
    let preview = Rect::new(
        inner.x + settings_width + 1,
        inner.y,
        inner.width.saturating_sub(settings_width + 1),
        inner.height,
    );
    (settings, preview)
}

/// Every clickable rectangle, plus the text rows to draw: (rect, label, hit).
struct Layout {
    buttons: Vec<(Rect, String, DialogHit)>,
    labels: Vec<(Rect, String, bool)>,
}

fn build(inner: Rect, settings: &PaddingSettings, state: &DialogState) -> Layout {
    let (column, _) = columns(inner);
    let mut layout = Layout {
        buttons: Vec::new(),
        labels: Vec::new(),
    };
    let mut y = column.y;
    let right = column.right();
    let row = |y: u16| Rect::new(column.x, y, column.width, 1);

    let section = |layout: &mut Layout, y: &mut u16, title: &str, is_tree: bool| {
        let panel = section_panel(state, settings, is_tree);
        layout.labels.push((row(*y), title.to_owned(), true));
        *y += 1;
        if !is_tree {
            let shared = settings.right_panels_shared;
            let text = format!(
                "{} Same for terminal, editor and board",
                boolean_switch(shared)
            );
            layout
                .buttons
                .push((row(*y), text, DialogHit::Shared(!shared)));
            *y += 1;
            if !shared {
                let mut x = column.x;
                for kind in PaddingPanel::RIGHT_KINDS {
                    let label = format!(" {} ", kind.label());
                    let width = label.chars().count() as u16;
                    if x + width <= right {
                        layout.buttons.push((
                            Rect::new(x, *y, width, 1),
                            label,
                            DialogHit::RightType(kind),
                        ));
                    }
                    x += width + 1;
                }
                *y += 1;
            }
        }
        let config = *settings.panel(panel);
        let linked_text = format!("{} Link all sides", boolean_switch(config.linked));
        layout
            .buttons
            .push((row(*y), linked_text, DialogHit::Linked(panel)));
        *y += 1;
        let value_row = |layout: &mut Layout,
                         y: &mut u16,
                         name: &str,
                         side: Option<PaddingSide>,
                         current: u8| {
            layout.labels.push((
                Rect::new(column.x, *y, 8.min(column.width), 1),
                name.to_owned(),
                false,
            ));
            for cells in 0..=MAX_PADDING {
                let x = column.x + 9 + u16::from(cells) * (BUTTON_WIDTH + 1);
                if x + BUTTON_WIDTH <= right {
                    let label = format!(" {cells} ");
                    layout.buttons.push((
                        Rect::new(x, *y, BUTTON_WIDTH, 1),
                        if cells == current {
                            format!("[{cells}]")
                        } else {
                            label
                        },
                        DialogHit::Value { panel, side, cells },
                    ));
                }
            }
            *y += 1;
        };
        if config.linked {
            value_row(layout, y, "All", None, config.all);
        } else {
            for side in PaddingSide::ALL {
                value_row(layout, y, side.label(), Some(side), side.get(config.sides));
            }
        }
        *y += 1;
    };

    section(&mut layout, &mut y, "Left panel (tree)", true);
    section(
        &mut layout,
        &mut y,
        "Right panel (agents, terminals)",
        false,
    );

    let close = " Done ";
    let width = close.chars().count() as u16;
    let close_y = (inner.bottom().saturating_sub(1)).max(y);
    layout.buttons.push((
        Rect::new(column.x, close_y, width, 1),
        close.into(),
        DialogHit::Close,
    ));
    layout
}

pub fn hit(
    screen: Rect,
    settings: &PaddingSettings,
    state: &DialogState,
    position: Position,
) -> Option<DialogHit> {
    let inner = Block::default()
        .borders(Borders::ALL)
        .inner(dialog_area(screen));
    build(inner, settings, state)
        .buttons
        .into_iter()
        .find_map(|(rect, _, target)| rect.contains(position).then_some(target))
}

pub fn render(frame: &mut Frame, screen: Rect, app: &App, state: &DialogState) {
    let area = dialog_area(screen);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(true))
        .title(theme::chrome_title("Padding fine-tuning"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let settings = &app.padding_settings;
    let layout = build(inner, settings, state);

    for (rect, text, is_title) in &layout.labels {
        let style = if *is_title {
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        frame.render_widget(Paragraph::new(Span::styled(text.clone(), style)), *rect);
    }
    for (rect, text, target) in &layout.buttons {
        let is_hovered = state.hover == Some(*target);
        let is_chosen = match target {
            DialogHit::Value { panel, side, cells } => {
                let config = settings.panel(*panel);
                let current = match side {
                    None => config.all,
                    Some(side) => side.get(config.sides),
                };
                current == *cells
            }
            DialogHit::RightType(kind) => *kind == state.right_type,
            _ => false,
        };
        let style = if is_chosen {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else if is_hovered {
            Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else if matches!(
            target,
            DialogHit::Value { .. } | DialogHit::RightType(_) | DialogHit::Close
        ) {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::DIM)
        } else {
            Style::new()
        };
        let alignment = if matches!(target, DialogHit::Value { .. }) {
            Alignment::Center
        } else {
            Alignment::Left
        };
        frame.render_widget(
            Paragraph::new(Span::styled(text.clone(), style)).alignment(alignment),
            *rect,
        );
    }

    let (_, preview) = columns(inner);
    render_preview(frame, preview, settings, state);
}

/// Draws one dummy panel: border, dotted padding cells, neutral content
/// inside the padding.
fn dummy_panel(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    padding: crate::padding::PaddingSides,
    lines: &[&str],
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style(false))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().add_modifier(Modifier::DIM),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let dots = vec![Line::from("·".repeat(usize::from(inner.width))); usize::from(inner.height)];
    frame.render_widget(
        Paragraph::new(dots).style(Style::new().add_modifier(Modifier::DIM)),
        inner,
    );
    let content = padding.inset(inner);
    let text: Vec<Line> = lines.iter().map(|line| Line::from(*line)).collect();
    frame.render_widget(Clear, content);
    frame.render_widget(Paragraph::new(text), content);
}

fn render_preview(frame: &mut Frame, area: Rect, settings: &PaddingSettings, state: &DialogState) {
    if area.width < 12 || area.height < 5 {
        return;
    }
    let frame_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " Preview (dots are padding) ",
            Style::new().add_modifier(Modifier::DIM),
        ));
    let inner = frame_block.inner(area);
    frame.render_widget(frame_block, area);
    let tree_width = (inner.width / 3)
        .clamp(8, 30)
        .min(inner.width.saturating_sub(4));
    let tree_area = Rect::new(inner.x, inner.y, tree_width, inner.height);
    let agent_area = Rect::new(
        inner.x + tree_width,
        inner.y,
        inner.width - tree_width,
        inner.height,
    );
    let tree_padding = settings.panel(PaddingPanel::Tree).resolved();
    let agent_padding = settings
        .panel(section_panel(state, settings, false))
        .resolved();
    dummy_panel(
        frame,
        tree_area,
        "Tree",
        tree_padding,
        &[
            "▾ Project",
            "  ● Agent one",
            "  ○ Agent two",
            "  ○ Shell",
            "▸ Notes",
        ],
    );
    dummy_panel(
        frame,
        agent_area,
        "Agent",
        agent_padding,
        &[
            "> Summarize the open tasks",
            "",
            "Here are three open tasks:",
            "  1. Review the draft",
            "  2. Update the notes",
            "  3. Send the summary",
            "",
            "> _",
        ],
    );
}

/// Applies a click inside the dialog. Returns `false` when the dialog should
/// close.
pub fn click(app: &mut App, state: &mut DialogState, target: DialogHit) -> bool {
    match target {
        DialogHit::Close => return false,
        DialogHit::Linked(panel) => {
            let linked = app.padding_settings.panel(panel).linked;
            app.padding_set_linked(panel, !linked);
        }
        DialogHit::Shared(shared) => app.padding_set_right_panels_shared(shared),
        DialogHit::RightType(kind) => state.right_type = kind,
        DialogHit::Value {
            panel,
            side: None,
            cells,
        } => app.padding_set_all(panel, cells),
        DialogHit::Value {
            panel,
            side: Some(side),
            cells,
        } => {
            app.padding_set_side(panel, side, cells);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::padding::PaddingMode;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> (App, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("padding-dialog".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.padding_settings.mode = PaddingMode::Enabled;
        (app, directory)
    }

    fn screen() -> Rect {
        Rect::new(0, 0, 140, 40)
    }

    fn click_at(app: &mut App, state: &mut DialogState, wanted: DialogHit) {
        let inner = Block::default()
            .borders(Borders::ALL)
            .inner(dialog_area(screen()));
        let layout = build(inner, &app.padding_settings, state);
        let (rect, _, _) = layout
            .buttons
            .iter()
            .find(|(_, _, target)| *target == wanted)
            .unwrap_or_else(|| panic!("{wanted:?} not shown"));
        let found = hit(screen(), &app.padding_settings, state, rect.as_position());
        assert_eq!(found, Some(wanted));
        click(app, state, wanted);
    }

    #[test]
    fn linking_hides_the_individual_sides() {
        let (mut app, _directory) = app();
        let state = DialogState::default();
        let inner = Block::default()
            .borders(Borders::ALL)
            .inner(dialog_area(screen()));
        let linked = build(inner, &app.padding_settings, &state);
        assert!(!linked.labels.iter().any(|(_, text, _)| text == "Top"));
        app.padding_set_linked(PaddingPanel::Tree, false);
        let unlinked = build(inner, &app.padding_settings, &state);
        assert!(unlinked.labels.iter().any(|(_, text, _)| text == "Top"));
    }

    #[test]
    fn value_buttons_set_all_and_single_sides() {
        let (mut app, _directory) = app();
        let mut state = DialogState::default();
        click_at(
            &mut app,
            &mut state,
            DialogHit::Value {
                panel: PaddingPanel::Tree,
                side: None,
                cells: 3,
            },
        );
        assert_eq!(app.padding_settings.tree.all, 3);
        click_at(&mut app, &mut state, DialogHit::Linked(PaddingPanel::Tree));
        click_at(
            &mut app,
            &mut state,
            DialogHit::Value {
                panel: PaddingPanel::Tree,
                side: Some(PaddingSide::Bottom),
                cells: 0,
            },
        );
        assert_eq!(app.padding_settings.tree.sides.bottom, 0);
    }

    #[test]
    fn per_type_settings_appear_only_when_not_shared() {
        let (mut app, _directory) = app();
        let mut state = DialogState::default();
        let inner = Block::default()
            .borders(Borders::ALL)
            .inner(dialog_area(screen()));
        assert!(!build(inner, &app.padding_settings, &state)
            .buttons
            .iter()
            .any(|(_, _, target)| matches!(target, DialogHit::RightType(_))));
        click_at(&mut app, &mut state, DialogHit::Shared(false));
        click_at(
            &mut app,
            &mut state,
            DialogHit::RightType(PaddingPanel::Editor),
        );
        click_at(
            &mut app,
            &mut state,
            DialogHit::Value {
                panel: PaddingPanel::Editor,
                side: None,
                cells: 2,
            },
        );
        assert_eq!(app.padding_settings.editor.all, 2);
        assert_ne!(app.padding_settings.terminal.all, 2);
        assert!(!click(&mut app, &mut state, DialogHit::Close));
    }

    #[test]
    fn dialog_renders_settings_and_preview() {
        let (app, _directory) = app();
        let state = DialogState::default();
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..40)
            .map(|y| {
                (0..140)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "Padding fine-tuning",
            "[ON  ●] Link all sides",
            "Link all sides",
            "Preview",
            "Project",
            "Summarize",
            "Done",
        ] {
            assert!(text.contains(needle), "missing {needle}\n{text}");
        }
    }
}
