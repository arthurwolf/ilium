//! The Padding settings tab: three selectable boxes (Disabled, Size
//! dependent, Enabled) side by side, a threshold input inside the size-dependent box
//! and, once padding can apply, the "Padding fine-tuning" button.
//!
//! [`tab_layout`] is the single source of geometry: [`render`] paints from it
//! and [`hit`] hit-tests against it, so drawing and clicking cannot disagree.
//! The tab never scrolls; the layout is deliberately compact.

use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, SettingsState};
use crate::padding::{PaddingMode, PaddingSettings};
use crate::theme;
use crate::value_control::{
    ControlAction, ControlKind, ControlSpec, ControlStyles, PointerButton, ValueControl,
};

/// Height of every mode box, borders included.
pub const CARD_HEIGHT: u16 = 10;
const CARD_GAP: u16 = 1;
const SIDE_MARGIN: u16 = 2;
const INTRO_ROWS: u16 = 4;
/// Cells reserved for the threshold value: four digits (the largest value the
/// field accepts is 1000). A fixed width keeps the shared editor's control
/// geometry stable.
const THRESHOLD_TEXT_CELLS: usize = 5;
const SUMMARY_ROWS: usize = 4;
const FINE_TUNE_LABEL: &str = " Padding fine-tuning ";

/// Keyboard rows of the tab, in `selected_row` order.
pub const ROW_MODE: usize = 0;
pub const ROW_THRESHOLD: usize = 1;
pub const ROW_FINE_TUNE: usize = 2;

/// A clickable target of the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddingHit {
    /// A mode box (anywhere on it that is not a control).
    Mode(PaddingMode),
    /// The threshold's "-" or "+" button, by the number of columns it moves.
    Step(i32),
    /// The threshold's value (opens the shared exact number editor).
    Field,
    /// The "Padding fine-tuning" button.
    FineTune,
}

/// Transient, never-persisted state of the tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaddingUiState {
    pub hover: Option<PaddingHit>,
    pub dialog: Option<crate::padding_dialog::DialogState>,
}

/// Where everything sits inside the tab's content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabLayout {
    pub cards: [Rect; 3],
    /// The threshold's number control row inside the size-dependent card.
    pub threshold: Rect,
    pub fine_tune: Option<Rect>,
}

pub fn tab_layout(area: Rect, settings: &PaddingSettings) -> TabLayout {
    let inner_x = area.x.saturating_add(SIDE_MARGIN);
    let inner_width = area.width.saturating_sub(SIDE_MARGIN * 2);
    let card_width = inner_width.saturating_sub(CARD_GAP * 2) / 3;
    let top = area.y.saturating_add(INTRO_ROWS);
    let card = |index: u16| {
        Rect::new(
            inner_x + index * (card_width + CARD_GAP),
            top,
            card_width,
            CARD_HEIGHT,
        )
    };
    let cards = [card(0), card(1), card(2)];

    let inside = Block::default().borders(Borders::ALL).inner(cards[1]);
    let input_row = inside.bottom().saturating_sub(1);
    let threshold = Rect::new(inside.x, input_row, inside.width, 1);

    let button_width = FINE_TUNE_LABEL.width() as u16;
    let fine_tune = (settings.mode != PaddingMode::Disabled).then(|| {
        Rect::new(
            inner_x,
            top + CARD_HEIGHT + 1,
            button_width.min(inner_width),
            1,
        )
    });

    TabLayout {
        cards,
        threshold,
        fine_tune,
    }
}

/// The threshold's shared number control uses a fixed value width so pointer
/// targets retain their geometry while the shared editor is open.
fn threshold_control(row: Rect, threshold: u16) -> ValueControl {
    let padded = format!("{threshold:^width$}", width = THRESHOLD_TEXT_CELLS);
    ValueControl::new(
        row,
        ControlSpec {
            kind: ControlKind::Number,
            label: "",
            value: &padded,
            label_width: 0,
            previous_enabled: threshold > crate::padding::MIN_WIDTH_THRESHOLD,
            next_enabled: threshold < crate::padding::MAX_WIDTH_THRESHOLD,
            open_enabled: true,
        },
    )
}

/// What is under `position`, if anything.
pub fn hit(area: Rect, settings: &PaddingSettings, position: Position) -> Option<PaddingHit> {
    let layout = tab_layout(area, settings);
    if let Some(button) = layout.fine_tune {
        if button.contains(position) {
            return Some(PaddingHit::FineTune);
        }
    }
    if layout.threshold.contains(position) {
        // Inert spots in the number row do not fall through to the mode card.
        let action = threshold_control(layout.threshold, settings.width_threshold)
            .hit(position, PointerButton::Left);
        return match action {
            Some(ControlAction::Decrement) => Some(PaddingHit::Step(-1)),
            Some(ControlAction::Increment) => Some(PaddingHit::Step(1)),
            Some(ControlAction::EditNumber) => Some(PaddingHit::Field),
            _ => None,
        };
    }
    PaddingMode::ALL
        .into_iter()
        .zip(layout.cards)
        .find_map(|(mode, card)| card.contains(position).then_some(PaddingHit::Mode(mode)))
}

/// Greedy word wrap by display width; a word wider than `width` is kept whole
/// and clipped by the caller's rectangle.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed = if current.is_empty() {
            word.width()
        } else {
            current.width() + 1 + word.width()
        };
        if needed > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn card_border_style(is_selected: bool, is_hovered: bool) -> Style {
    if is_selected {
        Style::new()
            .fg(theme::accent_bg())
            .add_modifier(Modifier::BOLD)
    } else if is_hovered {
        theme::border_style(true)
    } else {
        theme::border_style(false)
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    let settings = &app.padding_settings;
    let ui = &state.padding;
    let layout = tab_layout(area, settings);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "Padding",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Empty cells between a side panel's border and its content.",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ])
        .style(Style::new()),
        Rect::new(
            area.x.saturating_add(SIDE_MARGIN),
            area.y,
            area.width.saturating_sub(SIDE_MARGIN),
            INTRO_ROWS - 1,
        ),
    );

    for (index, (mode, card)) in PaddingMode::ALL.into_iter().zip(layout.cards).enumerate() {
        let is_selected = settings.mode == mode;
        let is_hovered = matches!(
            ui.hover,
            Some(PaddingHit::Mode(hovered)) if hovered == mode
        ) || (mode == PaddingMode::WidthDependent
            && matches!(ui.hover, Some(PaddingHit::Step(_) | PaddingHit::Field)));
        let is_focused = state.selected_row == ROW_MODE && is_selected;
        let marker = if is_selected { "●" } else { "○" };
        let title_style = if is_selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else if is_hovered {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(card_border_style(is_selected, is_hovered))
            .title(Line::from(Span::styled(
                format!(" {marker} {} ", mode.label()),
                title_style,
            )));
        let inside = block.inner(card);
        frame.render_widget(block, card);
        let summary_style = if is_selected || is_hovered {
            Style::new()
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        let summary: Vec<Line> = wrap(mode.summary(), usize::from(inside.width.max(1)))
            .into_iter()
            .take(SUMMARY_ROWS)
            .map(|line| Line::from(Span::styled(line, summary_style)))
            .collect();
        frame.render_widget(
            Paragraph::new(summary),
            Rect::new(
                inside.x,
                inside.y.saturating_add(1),
                inside.width,
                SUMMARY_ROWS as u16,
            ),
        );
        if is_focused && index < 3 {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "◂ ▸ arrows",
                    Style::new().add_modifier(Modifier::DIM),
                ))
                .alignment(Alignment::Right),
                Rect::new(inside.x, inside.y, inside.width, 1),
            );
        }
        if mode == PaddingMode::WidthDependent {
            render_threshold(frame, inside, &layout, app, state);
        }
    }

    if let Some(button) = layout.fine_tune {
        let is_hovered =
            ui.hover == Some(PaddingHit::FineTune) || state.selected_row == ROW_FINE_TUNE;
        let style = if is_hovered {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::REVERSED)
        };
        frame.render_widget(Paragraph::new(Span::styled(FINE_TUNE_LABEL, style)), button);
    }
}

fn render_threshold(
    frame: &mut Frame,
    inside: Rect,
    layout: &TabLayout,
    app: &App,
    state: &SettingsState,
) {
    let settings = &app.padding_settings;
    let ui = &state.padding;
    let is_active = settings.mode == PaddingMode::WidthDependent;
    let label_style = if is_active {
        Style::new()
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let label = if inside.width < 17 {
        "Columns:"
    } else {
        "Terminal columns:"
    };
    frame.render_widget(
        Paragraph::new(Span::styled(label, label_style)),
        Rect::new(
            inside.x,
            layout.threshold.y.saturating_sub(1),
            inside.width,
            1,
        ),
    );
    let is_highlighted = ui.hover == Some(PaddingHit::Field)
        || ui.hover == Some(PaddingHit::Step(-1))
        || ui.hover == Some(PaddingHit::Step(1))
        || state.selected_row == ROW_THRESHOLD;
    let value_style = if is_highlighted {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    let button_style = if is_active {
        Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::DIM)
    };
    threshold_control(layout.threshold, settings.width_threshold).render(
        frame,
        ControlStyles {
            background: Style::new(),
            label: label_style,
            value: value_style,
            button: button_style,
            disabled: Style::new().add_modifier(Modifier::DIM),
        },
    );
}

/// Hover tracking for `Moved` events.
pub fn update_hover(area: Rect, app: &App, state: &mut SettingsState, position: Position) {
    state.padding.hover = hit(area, &app.padding_settings, position);
}

/// Applies a click on the tab.
pub fn click(app: &mut App, state: &mut SettingsState, target: PaddingHit) {
    match target {
        PaddingHit::Mode(mode) => {
            state.selected_row = ROW_MODE;
            app.padding_set_mode(mode);
        }
        PaddingHit::Step(delta) => {
            state.selected_row = ROW_THRESHOLD;
            app.step_settings_number(
                crate::value_settings::SettingsNumber::PaddingThreshold,
                delta,
            );
        }
        PaddingHit::Field => {
            state.selected_row = ROW_THRESHOLD;
            // Caller opens the shared number dialog after restoring Settings mode.
        }
        PaddingHit::FineTune => {
            state.selected_row = ROW_FINE_TUNE;
            state.padding.dialog = Some(crate::padding_dialog::DialogState::default());
        }
    }
}

/// Keyboard handling for the tab. Returns whether the key was used.
pub fn handle_key(
    app: &mut App,
    state: &mut SettingsState,
    key: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::KeyCode;

    let rows = if app.padding_settings.mode == PaddingMode::Disabled {
        2
    } else {
        3
    };
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.selected_row = (state.selected_row + 1).min(rows - 1);
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
            let direction: i32 = if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                -1
            } else {
                1
            };
            match state.selected_row {
                ROW_MODE => {
                    let index = PaddingMode::ALL
                        .iter()
                        .position(|mode| *mode == app.padding_settings.mode)
                        .unwrap_or(0) as i32;
                    let next = (index + direction).clamp(0, 2) as usize;
                    app.padding_set_mode(PaddingMode::ALL[next]);
                }
                ROW_THRESHOLD => return false,
                _ => {}
            }
        }
        KeyCode::Char('-' | '+') if state.selected_row == ROW_THRESHOLD => return false,
        // PageUp/PageDown keep the established ten-column step through the same saver.
        KeyCode::PageUp | KeyCode::PageDown if state.selected_row == ROW_THRESHOLD => {
            app.step_padding_threshold_by(if key.code == KeyCode::PageUp { 10 } else { -10 });
        }
        KeyCode::Enter | KeyCode::Char(' ') => match state.selected_row {
            ROW_THRESHOLD => return false,
            ROW_FINE_TUNE => {
                state.padding.dialog = Some(crate::padding_dialog::DialogState::default());
            }
            _ => {}
        },
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> (App, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("padding-ui".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        (app, directory)
    }

    fn draw(app: &App, state: &SettingsState, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), app, state))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn three_boxes_share_the_width_without_overlapping() {
        let settings = PaddingSettings::default();
        let layout = tab_layout(Rect::new(0, 0, 90, 30), &settings);
        for pair in layout.cards.windows(2) {
            assert!(pair[0].right() < pair[1].x);
            assert_eq!(pair[0].width, pair[1].width);
        }
        assert!(layout.fine_tune.is_none(), "hidden while disabled");
        assert!(layout.cards[1].contains(layout.threshold.as_position()));
    }

    #[test]
    fn boxes_render_titles_and_selection_marker() {
        let (mut app, _directory) = app();
        let state = SettingsState::default();
        let text = draw(&app, &state, 90, 20);
        assert!(text.contains("● Disabled"), "{text}");
        assert!(text.contains("○ Enabled"));
        assert!(text.contains("○ Size dependent"));
        assert!(text.contains("200"));
        assert!(!text.contains("Padding fine-tuning"));
        app.padding_settings.mode = PaddingMode::Enabled;
        let text = draw(&app, &state, 90, 20);
        assert!(text.contains("● Enabled"));
        assert!(text.contains("Padding fine-tuning"));
    }

    #[test]
    fn clicking_boxes_steppers_and_the_button_changes_state() {
        let (mut app, _directory) = app();
        let mut state = SettingsState::default();
        let area = Rect::new(0, 0, 90, 20);
        let layout = tab_layout(area, &app.padding_settings);

        let target = hit(area, &app.padding_settings, layout.cards[2].as_position()).unwrap();
        assert_eq!(target, PaddingHit::Mode(PaddingMode::Enabled));
        click(&mut app, &mut state, target);
        assert_eq!(app.padding_settings.mode, PaddingMode::Enabled);

        let layout = tab_layout(area, &app.padding_settings);
        let button = hit(
            area,
            &app.padding_settings,
            layout.fine_tune.unwrap().as_position(),
        );
        assert_eq!(button, Some(PaddingHit::FineTune));
        click(&mut app, &mut state, PaddingHit::FineTune);
        assert!(state.padding.dialog.is_some());
        state.padding.dialog = None;

        let control = threshold_control(layout.threshold, app.padding_settings.width_threshold);
        let increment = control.geometry().next.as_position();
        let step = hit(area, &app.padding_settings, increment).unwrap();
        assert_eq!(step, PaddingHit::Step(1));
        click(&mut app, &mut state, step);
        assert_eq!(app.padding_settings.mode, PaddingMode::WidthDependent);
        assert_eq!(app.padding_settings.width_threshold, 201);
        let decrement = control.geometry().previous.as_position();
        let step = hit(area, &app.padding_settings, decrement);
        assert_eq!(step, Some(PaddingHit::Step(-1)));
        click(&mut app, &mut state, step.unwrap());
        assert_eq!(app.padding_settings.width_threshold, 200);
    }

    #[test]
    fn threshold_keys_are_delegated_to_shared_numeric_settings() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut app, _directory) = app();
        let mut state = SettingsState {
            selected_row: ROW_THRESHOLD,
            ..SettingsState::default()
        };
        let press = |app: &mut App, state: &mut SettingsState, code: KeyCode| {
            handle_key(app, state, KeyEvent::new(code, KeyModifiers::NONE))
        };
        assert!(!press(&mut app, &mut state, KeyCode::Char('-')));
        assert!(!press(&mut app, &mut state, KeyCode::Char('+')));
        assert!(press(&mut app, &mut state, KeyCode::PageDown));
        assert_eq!(app.padding_settings.width_threshold, 190);
        assert!(press(&mut app, &mut state, KeyCode::PageUp));
        assert_eq!(app.padding_settings.width_threshold, 200);
        assert!(!press(&mut app, &mut state, KeyCode::Char('*')));
        assert_eq!(app.padding_settings.width_threshold, 200);
    }

    #[test]
    fn threshold_buttons_are_left_click_targets_and_value_opens_shared_editor() {
        let settings = PaddingSettings::default();
        let area = Rect::new(0, 0, 90, 20);
        let layout = tab_layout(area, &settings);
        let control = threshold_control(layout.threshold, settings.width_threshold);
        let geometry = control.geometry();
        assert_eq!(
            hit(area, &settings, geometry.previous.as_position()),
            Some(PaddingHit::Step(-1))
        );
        assert_eq!(
            hit(area, &settings, geometry.next.as_position()),
            Some(PaddingHit::Step(1))
        );
        assert_eq!(
            hit(area, &settings, geometry.open.as_position()),
            Some(PaddingHit::Field)
        );
        let ink = control.value_ink()[0];
        assert_eq!(
            hit(area, &settings, ink.as_position()),
            Some(PaddingHit::Field)
        );
        assert_eq!(
            control.hit(geometry.previous.as_position(), PointerButton::Right),
            None,
            "only the left button steps"
        );
    }

    #[test]
    fn threshold_glyphs_are_plain_minus_plus_and_star() {
        let (mut app, _directory) = app();
        app.padding_settings.mode = PaddingMode::WidthDependent;
        let state = SettingsState::default();
        let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &state))
            .unwrap();
        let layout = tab_layout(Rect::new(0, 0, 90, 20), &app.padding_settings);
        let geometry =
            threshold_control(layout.threshold, app.padding_settings.width_threshold).geometry();
        for (rect, glyph) in [
            (geometry.previous, "-"),
            (geometry.next, "+"),
            (geometry.open, "*"),
        ] {
            assert_eq!(
                terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                glyph
            );
        }
    }

    fn send_key(app: &mut App, code: crossterm::event::KeyCode) {
        crate::keys::handle_event(
            app,
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                code,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
    }

    fn open_threshold_editor(app: &mut App, code: crossterm::event::KeyCode) {
        app.mode = crate::app::Mode::Settings(Box::new(SettingsState {
            tab: crate::app::SettingsTab::Padding,
            selected_row: ROW_THRESHOLD,
            ..SettingsState::default()
        }));
        send_key(app, code);
        assert!(matches!(app.mode, crate::app::Mode::ValueDialog(_)));
    }

    #[test]
    fn threshold_keyboard_aliases_keep_step_and_space_open_behavior() {
        use crossterm::event::KeyCode;
        let (mut app, directory) = app();
        app.padding_set_mode(PaddingMode::WidthDependent);
        app.padding_set_threshold(200).unwrap();
        app.settle_filesystem_for_test();

        app.mode = crate::app::Mode::Settings(Box::new(SettingsState {
            tab: crate::app::SettingsTab::Padding,
            selected_row: ROW_THRESHOLD,
            ..SettingsState::default()
        }));
        send_key(&mut app, KeyCode::Char('h'));
        assert_eq!(app.padding_settings.width_threshold, 199);
        send_key(&mut app, KeyCode::Char('l'));
        assert_eq!(app.padding_settings.width_threshold, 200);
        app.settle_filesystem_for_test();
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .padding
                .width_threshold,
            200
        );

        send_key(&mut app, KeyCode::Char(' '));
        assert!(matches!(app.mode, crate::app::Mode::ValueDialog(_)));
        if let crate::app::Mode::ValueDialog(host) = &app.mode {
            let crate::value_dialog::ValueDialogState::Number(number) = &host.dialog else {
                panic!("Space must open the shared number editor");
            };
            assert_eq!(number.draft.buf, "200");
        }
    }

    #[test]
    fn pointer_field_dispatch_opens_the_shared_number_dialog() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::layout::Rect;

        let (mut app, _directory) = app();
        app.padding_settings.mode = PaddingMode::WidthDependent;
        let screen = Rect::new(0, 0, 120, 40);
        app.set_screen_area(screen);
        let state = SettingsState {
            tab: crate::app::SettingsTab::Padding,
            ..SettingsState::default()
        };
        let mut settings_layout = crate::settings_ui::compute_layout_for_mode(screen, &app, &state);
        let instruction_height = crate::settings_ui::instructions_panel_height(
            &app,
            state.tab,
            settings_layout.content_area,
        );
        settings_layout.content_area.y += instruction_height;
        settings_layout.content_area.height = settings_layout
            .content_area
            .height
            .saturating_sub(instruction_height);
        let padding_layout = tab_layout(settings_layout.content_area, &app.padding_settings);
        let editor = threshold_control(
            padding_layout.threshold,
            app.padding_settings.width_threshold,
        );
        let position = editor.geometry().open.as_position();
        app.mode = crate::app::Mode::Settings(Box::new(state));

        let emitted = app.capture_emitted_geometry(1);
        app.commit_emitted_geometry(emitted);
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(matches!(app.mode, crate::app::Mode::ValueDialog(_)));
    }

    fn replace_editor_text(app: &mut App, old: &str, new: &str) {
        for _ in old.chars() {
            send_key(app, crossterm::event::KeyCode::Backspace);
        }
        for character in new.chars() {
            send_key(app, crossterm::event::KeyCode::Char(character));
        }
    }

    #[test]
    fn shared_threshold_editor_cancel_and_enter_are_deferred_for_every_mode() {
        use crossterm::event::KeyCode;

        for mode in PaddingMode::ALL {
            let (mut app, directory) = app();
            app.padding_set_mode(mode);
            app.padding_set_threshold(200).unwrap();
            app.settle_filesystem_for_test();
            let accepted = app.padding_settings;

            // `*` opens exact entry without first forcing WidthDependent mode.
            open_threshold_editor(&mut app, KeyCode::Char('*'));
            replace_editor_text(&mut app, "200", "150");
            assert_eq!(app.padding_settings, accepted, "typing mutated {mode:?}");
            send_key(&mut app, KeyCode::Esc);
            app.settle_filesystem_for_test();
            assert_eq!(app.padding_settings, accepted, "cancel mutated {mode:?}");
            assert_eq!(
                crate::config::load(directory.path()).unwrap().padding,
                accepted
            );

            // Enter opens the same shared dialog; only its Enter commits the value.
            open_threshold_editor(&mut app, KeyCode::Enter);
            replace_editor_text(&mut app, "200", "150");
            assert_eq!(app.padding_settings, accepted, "draft applied before Enter");
            send_key(&mut app, KeyCode::Enter);
            app.settle_filesystem_for_test();
            assert_eq!(app.padding_settings.mode, PaddingMode::WidthDependent);
            assert_eq!(app.padding_settings.width_threshold, 150);
            assert_eq!(
                crate::config::load(directory.path()).unwrap().padding,
                app.padding_settings
            );
        }
    }

    #[test]
    fn shared_threshold_editor_retains_invalid_drafts_and_corrects_exactly() {
        use crossterm::event::KeyCode;

        for invalid in ["", "10", "1001", "10000", "65536"] {
            let (mut app, directory) = app();
            app.padding_set_mode(PaddingMode::Enabled);
            app.padding_set_threshold(200).unwrap();
            app.settle_filesystem_for_test();
            let accepted = app.padding_settings;

            open_threshold_editor(&mut app, KeyCode::Char('*'));
            replace_editor_text(&mut app, "200", invalid);
            send_key(&mut app, KeyCode::Enter);
            let crate::app::Mode::ValueDialog(host) = &app.mode else {
                panic!("invalid draft {invalid:?} closed the shared editor");
            };
            let crate::value_dialog::ValueDialogState::Number(number) = &host.dialog else {
                panic!("padding editor is not numeric");
            };
            assert_eq!(number.draft.buf, invalid);
            assert!(number.error.is_some());
            app.settle_filesystem_for_test();
            assert_eq!(app.padding_settings, accepted);
            assert_eq!(
                crate::config::load(directory.path()).unwrap().padding,
                accepted
            );

            replace_editor_text(&mut app, invalid, "153");
            send_key(&mut app, KeyCode::Enter);
            app.settle_filesystem_for_test();
            assert_eq!(app.padding_settings.mode, PaddingMode::WidthDependent);
            assert_eq!(app.padding_settings.width_threshold, 153);
            assert_eq!(
                crate::config::load(directory.path()).unwrap().padding,
                app.padding_settings
            );
        }
    }

    #[test]
    fn threshold_arrows_at_domain_bounds_are_disabled() {
        for (value, disabled_action) in [
            (
                crate::padding::MIN_WIDTH_THRESHOLD,
                ControlAction::Decrement,
            ),
            (
                crate::padding::MAX_WIDTH_THRESHOLD,
                ControlAction::Increment,
            ),
        ] {
            let control = threshold_control(Rect::new(0, 0, 30, 1), value);
            let geometry = control.geometry();
            let disabled = if disabled_action == ControlAction::Decrement {
                geometry.previous
            } else {
                geometry.next
            };
            assert_eq!(
                control.hit(Position::new(disabled.x, disabled.y), PointerButton::Left),
                None
            );
            assert_eq!(
                control.hit(
                    Position::new(geometry.open.x, geometry.open.y),
                    PointerButton::Left
                ),
                Some(ControlAction::EditNumber)
            );
        }
    }

    #[test]
    fn threshold_steps_at_bounds_are_noops_without_forcing_padding_mode() {
        for (threshold, direction) in [
            (crate::padding::MIN_WIDTH_THRESHOLD, -1),
            (crate::padding::MAX_WIDTH_THRESHOLD, 1),
        ] {
            let (mut app, _directory) = app();
            app.padding_set_threshold(threshold).unwrap();
            app.settle_filesystem_for_test();
            let before = app.configuration_admission.attempts;
            app.step_settings_number(
                crate::value_settings::SettingsNumber::PaddingThreshold,
                direction,
            );
            assert_eq!(app.padding_settings.mode, PaddingMode::Disabled);
            assert_eq!(app.padding_settings.width_threshold, threshold);
            assert_eq!(app.configuration_admission.attempts, before);
        }
    }

    #[test]
    fn compact_padding_content_keeps_the_threshold_label_readable() {
        let (mut app, _directory) = app();
        app.padding_settings.mode = PaddingMode::WidthDependent;
        // The Settings sidebar leaves 53 content columns at an 80-column screen.
        let text = draw(&app, &SettingsState::default(), 53, 20);
        assert!(text.contains("Columns:"), "compact units must fit: {text}");
        assert!(
            !text.contains("Terminal colu"),
            "do not clip the longer label: {text}"
        );
    }

    #[test]
    fn narrow_content_keeps_every_threshold_target() {
        let settings = PaddingSettings::default();
        let layout = tab_layout(Rect::new(0, 0, 60, 20), &settings);
        let geometry = threshold_control(layout.threshold, settings.width_threshold).geometry();
        assert_eq!(geometry.previous.width, 1);
        assert_eq!(geometry.next.width, 1);
        assert_eq!(geometry.open.width, 1);
        assert!(geometry.value.width >= 3, "the value stays readable");
    }
    #[test]
    fn disabled_threshold_arrows_do_not_fall_through_to_the_mode_card() {
        let area = Rect::new(0, 0, 90, 20);
        for (threshold, decrement) in [
            (crate::padding::MIN_WIDTH_THRESHOLD, true),
            (crate::padding::MAX_WIDTH_THRESHOLD, false),
        ] {
            let settings = PaddingSettings {
                width_threshold: threshold,
                ..PaddingSettings::default()
            };
            let row = tab_layout(area, &settings).threshold;
            let control = threshold_control(row, threshold);
            let geometry = control.geometry();
            let disabled = if decrement {
                geometry.previous
            } else {
                geometry.next
            };
            assert_eq!(hit(area, &settings, disabled.as_position()), None);
            assert_eq!(
                hit(area, &settings, geometry.open.as_position()),
                Some(PaddingHit::Field)
            );
        }
    }
}
