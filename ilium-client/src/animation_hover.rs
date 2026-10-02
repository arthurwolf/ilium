//! The hover popover of Settings -> Animations that explains why a choice
//! option (for example the GPU renderer) is disabled.
//!
//! The mouse path only records which row the pointer rests on; the client
//! tick reveals the popover once the pointer has been still for
//! `HOVER_DELAY`, the same split the context-menu submenu hover uses. Any key
//! or click, leaving the row, scrolling and leaving Settings dismiss it.
//! `popover_geometry` is a pure function of the screen, the controls panel
//! and the row, so the popover can be tested for staying on screen.

use crate::{
    animation_rows::{DisabledOption, RowModel},
    animation_settings_ui::{control_ink, layout, row_y, Scrolls},
    app::{App, Mode, SettingsTab},
    last_prompt_banner::wrap_lines,
};
use ratatui::{
    layout::Rect,
    style::Modifier,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

/// How long the pointer must rest on the row before the popover appears.
pub const HOVER_DELAY: Duration = Duration::from_millis(150);
/// Preferred and minimum popover widths, border included.
const MAXIMUM_WIDTH: u16 = 46;
const MINIMUM_SIDE_WIDTH: u16 = 24;
/// A popover needs a border, one text row and a border.
const MINIMUM_HEIGHT: u16 = 3;

/// The row the pointer rests on and whether the delay has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationHover {
    pub row: usize,
    pub since: Instant,
    pub is_shown: bool,
}

/// Where the popover goes and the wrapped text lines it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverGeometry {
    pub rectangle: Rect,
    pub lines: Vec<String>,
}

/// The popover title and body for the disabled options of one row.
pub fn popover_text(options: &[DisabledOption]) -> (String, String) {
    match options {
        [] => (String::new(), String::new()),
        [only] => (format!(" {} unavailable ", only.label), only.reason.clone()),
        many => (
            " Unavailable options ".to_owned(),
            many.iter()
                .map(|option| format!("{}: {}", option.label, option.reason))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

/// Places the popover for the row drawn at `anchor_y`. It goes to the right
/// of the controls `panel` (over the field) when that is wide enough, so it
/// never covers the row; otherwise above or below the row inside the panel
/// columns. The result always lies inside `area` and never includes the last
/// row, which belongs to the voice affordance. `None` when nothing fits.
pub fn popover_geometry(
    area: Rect,
    panel: Rect,
    anchor_y: u16,
    body: &str,
) -> Option<PopoverGeometry> {
    let usable_bottom = if area.height >= 3 {
        area.bottom() - 1
    } else {
        area.bottom()
    };
    let usable_height = usable_bottom.saturating_sub(area.y);
    if usable_height < MINIMUM_HEIGHT || area.width < 4 {
        return None;
    }
    let wrapped = |width: u16| wrap_lines(body, width.saturating_sub(2).max(1));

    // Beside the panel: the row stays fully visible.
    let side_x = panel.right().saturating_add(1);
    let side_room = area.right().saturating_sub(side_x);
    if side_room >= MINIMUM_SIDE_WIDTH {
        let width = side_room.min(MAXIMUM_WIDTH);
        let mut lines = wrapped(width);
        let height = (lines.len() as u16 + 2).min(usable_height);
        lines.truncate(usize::from(height - 2));
        let y = anchor_y
            .min(usable_bottom.saturating_sub(height))
            .max(area.y);
        return Some(PopoverGeometry {
            rectangle: Rect::new(side_x, y, width, height),
            lines,
        });
    }

    // Narrow screens: over the panel, on whichever side of the row has room.
    let width = panel.width.min(area.width).max(4);
    let x = panel.x.max(area.x);
    let space_below = usable_bottom.saturating_sub(anchor_y.saturating_add(1));
    let space_above = anchor_y.saturating_sub(area.y);
    let mut lines = wrapped(width);
    let wanted = lines.len() as u16 + 2;
    let is_below = if wanted <= space_below {
        true
    } else if wanted <= space_above {
        false
    } else {
        space_below >= space_above
    };
    let space = if is_below { space_below } else { space_above };
    if space < MINIMUM_HEIGHT {
        return None;
    }
    let height = wanted.min(space);
    lines.truncate(usize::from(height - 2));
    if lines.len() < wrapped(width).len() {
        if let Some(last) = lines.last_mut() {
            last.push('\u{2026}');
        }
    }
    let y = if is_below {
        anchor_y + 1
    } else {
        anchor_y - height
    };
    Some(PopoverGeometry {
        rectangle: Rect::new(x, y, width.min(area.right() - x), height),
        lines,
    })
}

/// Draws the popover of the hovered row, if it is shown and still valid.
pub fn render(frame: &mut Frame, area: Rect, app: &App, model: &RowModel, scroll: Scrolls) {
    let Some(hover) = app.animation_hover.filter(|hover| hover.is_shown) else {
        return;
    };
    let Some(view) = model.view(hover.row) else {
        return;
    };
    if view.disabled_options.is_empty() {
        return;
    }
    let Some(anchor_y) = row_y(area, model, hover.row, scroll) else {
        return;
    };
    let (title, body) = popover_text(&view.disabled_options);
    let Some(geometry) = popover_geometry(area, layout(area).panel, anchor_y, &body) else {
        return;
    };
    let ink = control_ink(app);
    frame.render_widget(Clear, geometry.rectangle);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(ink)
        .border_style(ink.add_modifier(Modifier::DIM));
    frame.render_widget(
        Paragraph::new(geometry.lines.join("\n"))
            .style(ink)
            .block(block),
        geometry.rectangle,
    );
}

impl App {
    /// Records where the pointer rests: `Some(row)` for a row with disabled
    /// options, `None` anywhere else. Resting on the same row keeps the
    /// original timestamp so the delay is measured from first arrival.
    pub fn set_animation_hover(&mut self, row: Option<usize>, now: Instant) {
        self.animation_hover = match (row, self.animation_hover) {
            (Some(row), Some(hover)) if hover.row == row => Some(hover),
            (Some(row), _) => Some(AnimationHover {
                row,
                since: now,
                is_shown: false,
            }),
            (None, _) => None,
        };
    }

    /// Dismisses the popover (any key or click, leaving the row).
    pub fn clear_animation_hover(&mut self) {
        self.animation_hover = None;
    }

    /// Reveals the popover after the pointer has rested long enough and
    /// drops it when Settings -> Animations is no longer on screen. Returns
    /// whether the picture changed.
    pub fn tick_animation_hover(&mut self, now: Instant) -> bool {
        let Some(mut hover) = self.animation_hover else {
            return false;
        };
        let is_on_screen = matches!(
            &self.mode,
            Mode::Settings(state)
                if state.tab == SettingsTab::Animations && !state.animation_fullscreen
        );
        if !is_on_screen {
            self.animation_hover = None;
            return hover.is_shown;
        }
        if hover.is_shown || now.saturating_duration_since(hover.since) < HOVER_DELAY {
            return false;
        }
        hover.is_shown = true;
        self.animation_hover = Some(hover);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_rows::AnimationRow;
    use crate::animation_settings_ui::{follow_selection, row_y, AnimationHit, Scrolls};
    use crate::app::SettingsState;
    use crate::background_animation::AnimationKind;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ilium_ambient::gpu::{
        gpu_availability, set_gpu_availability, GpuAvailability, GpuUnavailable,
    };
    use ilium_ambient::ControlValue;
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    static GPU_LOCK: Mutex<()> = Mutex::new(());

    /// Holds the process-wide GPU availability for one test and restores it.
    struct GpuGuard {
        _lock: MutexGuard<'static, ()>,
        previous: GpuAvailability,
    }

    impl Drop for GpuGuard {
        fn drop(&mut self) {
            set_gpu_availability(self.previous.clone());
        }
    }

    fn with_availability(value: GpuAvailability) -> GpuGuard {
        let lock = GPU_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = gpu_availability();
        set_gpu_availability(value);
        GpuGuard {
            _lock: lock,
            previous,
        }
    }

    fn unavailable(reason: GpuUnavailable) -> GpuGuard {
        with_availability(GpuAvailability::Unavailable(reason))
    }

    fn reasons() -> Vec<GpuUnavailable> {
        vec![
            GpuUnavailable::NotCompiled,
            GpuUnavailable::Checking,
            GpuUnavailable::NoVulkanLoader,
            GpuUnavailable::NoDriver,
            GpuUnavailable::SoftwareOnly {
                adapter: "llvmpipe (LLVM 19)".to_owned(),
            },
            GpuUnavailable::NoDeviceAccess,
            GpuUnavailable::Failed("device lost".to_owned()),
        ]
    }

    fn backend_app(width: u16, height: u16) -> (App, tempfile::TempDir) {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, width, height));
        app.animation_settings.kind = AnimationKind::DitheredWaves;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        (app, project)
    }

    fn content_area(app: &App) -> Rect {
        crate::settings_ui::compute_layout(app.layout.screen_area).content_area
    }

    fn backend_row(app: &App) -> usize {
        app.animation_row_model()
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::SceneControl("render_backend"))
            .expect("the waves scene has a render_backend row")
    }

    /// Selects the row and scrolls it into view; returns its screen y.
    fn select_backend_row(app: &mut App) -> u16 {
        let row = backend_row(app);
        let content = content_area(app);
        let model = app.animation_row_model();
        let Mode::Settings(state) = &mut app.mode else {
            panic!("Settings stays open");
        };
        state.selected_row = row;
        let scrolls = follow_selection(content, &model, row, Scrolls::of(state));
        scrolls.store(state);
        row_y(content, &model, row, scrolls).expect("row is visible")
    }

    fn selected_row(app: &App) -> usize {
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        state.selected_row
    }

    fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        crate::mouse::handle_mouse_event(
            app,
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    fn key(app: &mut App, code: KeyCode) {
        crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn draw(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        terminal
    }

    fn screen_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
    }

    fn backend_choice(app: &App) -> ControlValue {
        app.animation_settings
            .scene_control("render_backend")
            .expect("render_backend control")
            .value
    }

    /// Hovers the row label and lets the delay elapse.
    fn hover_until_shown(app: &mut App, y: u16) {
        let content = content_area(app);
        pointer(
            app,
            MouseEventKind::Moved,
            layout(content).controls.x + 2,
            y,
        );
        assert!(app.animation_hover.is_some(), "pointer rests on the row");
        assert!(
            !app.tick_animation_hover(Instant::now()),
            "not shown before the delay"
        );
        assert!(app.tick_animation_hover(Instant::now() + HOVER_DELAY * 2));
    }

    fn normalized(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel() {
        let _gpu = unavailable(GpuUnavailable::NoDriver);
        let (mut app, _project) = backend_app(140, 40);
        select_backend_row(&mut app);
        let model = app.animation_row_model();
        let view = model.view(backend_row(&app)).unwrap();
        assert_eq!(view.value, "Software (slow-mo)");
        assert_eq!(view.disabled_options.len(), 1);
        assert_eq!(view.disabled_options[0].label, "GPU");
        let terminal = draw(&mut app, 140, 40);
        let content = content_area(&app);
        let scroll = match &app.mode {
            Mode::Settings(state) => Scrolls::of(state),
            _ => Scrolls::default(),
        };
        let y = row_y(content, &model, backend_row(&app), scroll).unwrap();
        let rows = screen_rows(&terminal);
        let row_text = &rows[usize::from(y)];
        assert!(row_text.contains("Renderer"), "{row_text}");
        assert!(row_text.contains("[ Software"), "{row_text}");
        let panel = layout(content).panel;
        let marker_column = row_text
            .chars()
            .take(usize::from(panel.right()))
            .collect::<String>()
            .find("GPU")
            .expect("a GPU marker is drawn inside the panel");
        let buffer = terminal.backend().buffer();
        let marker_x = row_text[..marker_column].chars().count() as u16;
        assert!(
            buffer[(marker_x, y)].modifier.contains(Modifier::DIM),
            "the marker is dim"
        );
        assert!(!buffer[(layout(content).controls.x + 2, y)]
            .modifier
            .contains(Modifier::DIM));
    }

    #[test]
    fn hover_popover_shows_each_reason_text_inside_the_screen() {
        for reason in reasons() {
            let _gpu = unavailable(reason.clone());
            let (mut app, _project) = backend_app(80, 24);
            let y = select_backend_row(&mut app);
            hover_until_shown(&mut app, y);
            let terminal = draw(&mut app, 80, 24);
            let content = content_area(&app);
            let panel = layout(content).panel;
            let fix = reason.fix();
            let geometry = popover_geometry(content, panel, y, &fix).unwrap();
            let rectangle = geometry.rectangle;
            assert!(
                rectangle.right() <= 80 && rectangle.bottom() <= 23,
                "{rectangle:?}"
            );
            assert!(
                !rectangle.intersects(Rect::new(panel.x, y, panel.width, 1)),
                "the row stays visible: {rectangle:?} row y={y}"
            );
            assert_eq!(normalized(&geometry.lines.join(" ")), normalized(&fix));
            let rows = screen_rows(&terminal);
            for (offset, line) in geometry.lines.iter().enumerate() {
                let row = &rows[usize::from(rectangle.y) + 1 + offset];
                assert!(
                    row.contains(line.as_str()),
                    "{reason:?}: {line:?} in {row:?}"
                );
            }
            assert!(
                rows[usize::from(rectangle.y)].contains("GPU unavailable"),
                "title names the option"
            );
        }
    }

    #[test]
    fn popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel() {
        let _gpu = unavailable(GpuUnavailable::NotCompiled);
        let (mut app, _project) = backend_app(80, 24);
        let y = select_backend_row(&mut app);
        let content = content_area(&app);
        let before = draw(&mut app, 80, 24);
        pointer(
            &mut app,
            MouseEventKind::Moved,
            layout(content).controls.x + 2,
            y,
        );
        let resting = draw(&mut app, 80, 24);
        assert_eq!(
            screen_rows(&before),
            screen_rows(&resting),
            "hidden until the delay"
        );
        assert!(app.tick_animation_hover(Instant::now() + HOVER_DELAY * 2));
        assert!(app.animation_hover.is_some_and(|hover| hover.is_shown));
        // Moving within the row keeps it; another row dismisses it.
        pointer(
            &mut app,
            MouseEventKind::Moved,
            layout(content).controls.x + 5,
            y,
        );
        assert!(app.animation_hover.is_some_and(|hover| hover.is_shown));
        pointer(
            &mut app,
            MouseEventKind::Moved,
            layout(content).controls.x + 5,
            y - 1,
        );
        assert!(app.animation_hover.is_none(), "leaving the row dismisses");
        for dismissal in 0..3 {
            let y = select_backend_row(&mut app);
            hover_until_shown(&mut app, y);
            match dismissal {
                0 => key(&mut app, KeyCode::Down),
                1 => pointer(
                    &mut app,
                    MouseEventKind::Down(MouseButton::Left),
                    layout(content).controls.x + 2,
                    y,
                ),
                _ => pointer(
                    &mut app,
                    MouseEventKind::ScrollDown,
                    layout(content).controls.x + 2,
                    y,
                ),
            }
            assert!(app.animation_hover.is_none(), "dismissal {dismissal}");
            let cleared = draw(&mut app, 80, 24);
            assert!(
                !screen_rows(&cleared)
                    .iter()
                    .any(|row| row.contains("GPU unavailable ")),
                "dismissal {dismissal}"
            );
        }
    }

    #[test]
    fn nothing_appears_while_the_gpu_is_ready() {
        let _gpu = with_availability(GpuAvailability::Ready {
            adapter: "Test GPU".to_owned(),
        });
        let (mut app, _project) = backend_app(80, 24);
        let y = select_backend_row(&mut app);
        let model = app.animation_row_model();
        let view = model.view(backend_row(&app)).unwrap();
        assert!(view.disabled_options.is_empty());
        assert!(view.help.contains("Test GPU"), "help names the adapter");
        let content = content_area(&app);
        pointer(
            &mut app,
            MouseEventKind::Moved,
            layout(content).controls.x + 2,
            y,
        );
        assert!(app.animation_hover.is_none());
        assert!(!app.tick_animation_hover(Instant::now() + HOVER_DELAY * 2));
        let terminal = draw(&mut app, 80, 24);
        let rows = screen_rows(&terminal);
        assert!(!rows.iter().any(|row| row.contains("unavailable")));
        assert!(
            !rows[usize::from(y)].contains("GPU"),
            "no marker when usable"
        );
    }

    #[test]
    fn keyboard_and_click_skip_the_disabled_option_and_explain_why() {
        let _gpu = unavailable(GpuUnavailable::NoVulkanLoader);
        let (mut app, _project) = backend_app(80, 24);
        let y = select_backend_row(&mut app);
        let software = ControlValue::Index(0);
        assert_eq!(backend_choice(&app), software);
        for code in [
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Enter,
            KeyCode::Char(' '),
        ] {
            app.status_message = None;
            key(&mut app, code);
            assert_eq!(
                backend_choice(&app),
                software,
                "{code:?} must not select GPU"
            );
            assert!(
                app.status_message
                    .as_deref()
                    .is_some_and(|message| message.starts_with("GPU unavailable:")),
                "{code:?}: {:?}",
                app.status_message
            );
        }
        // The help line of the selected row leads with the same summary.
        let terminal = draw(&mut app, 80, 24);
        assert!(screen_rows(&terminal)
            .iter()
            .any(|row| row.contains("GPU unavailable: The Vulkan loader")));
        // A click on the row changes nothing and reports the reason.
        let content = content_area(&app);
        let model = app.animation_row_model();
        let marker_hit = (content.x..layout(content).panel.right()).find(|column| {
            matches!(
                crate::animation_settings_ui::hit(
                    content,
                    &model,
                    match &app.mode {
                        Mode::Settings(state) => Scrolls::of(state),
                        _ => Scrolls::default(),
                    },
                    ratatui::layout::Position::new(*column, y),
                ),
                Some(AnimationHit::DisabledOption(_))
            )
        });
        let marker_column = marker_hit.expect("the marker is clickable");
        app.status_message = None;
        let other_row = if selected_row(&app) > 0 { 0 } else { 1 };
        if let Mode::Settings(state) = &mut app.mode {
            state.selected_row = other_row;
        }
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            marker_column,
            y,
        );
        assert_eq!(backend_choice(&app), software);
        assert_eq!(selected_row(&app), backend_row(&app));
        assert!(app
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("GPU unavailable:")));
        app.status_message = None;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            layout(content).controls.x + 2,
            y,
        );
        assert_eq!(backend_choice(&app), software, "row click cannot pick GPU");
    }

    #[test]
    fn a_ready_gpu_can_be_selected_from_the_keyboard() {
        let _gpu = with_availability(GpuAvailability::Ready {
            adapter: "Test GPU".to_owned(),
        });
        let (mut app, _project) = backend_app(80, 24);
        select_backend_row(&mut app);
        key(&mut app, KeyCode::Right);
        assert_eq!(backend_choice(&app), ControlValue::Index(1));
    }

    #[test]
    fn popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size() {
        let body = GpuUnavailable::NoDriver.fix();
        for (width, height) in [(80, 24), (60, 16), (44, 12), (46, 10), (140, 40), (30, 8)] {
            let area = Rect::new(0, 2, width, height - 2);
            let panel = layout(area).panel;
            for anchor_y in area.y..area.bottom() {
                let Some(geometry) = popover_geometry(area, panel, anchor_y, &body) else {
                    continue;
                };
                let rectangle = geometry.rectangle;
                assert!(rectangle.width >= 4 && rectangle.height >= MINIMUM_HEIGHT);
                assert!(rectangle.x >= area.x && rectangle.right() <= area.right());
                assert!(rectangle.y >= area.y && rectangle.bottom() <= area.bottom());
                assert!(
                    area.height < 3 || rectangle.bottom() < area.bottom(),
                    "keeps the voice row free: {rectangle:?}"
                );
                assert!(
                    geometry.lines.len() <= usize::from(rectangle.height - 2),
                    "text fits the box"
                );
                let row = Rect::new(panel.x, anchor_y, panel.width, 1);
                assert!(
                    !rectangle.intersects(row),
                    "{width}x{height} y={anchor_y}: {rectangle:?} covers the row"
                );
            }
        }
    }

    #[test]
    fn a_wide_screen_puts_the_popover_beside_the_panel() {
        let area = Rect::new(0, 2, 140, 38);
        let panel = layout(area).panel;
        let body = GpuUnavailable::NoDriver.fix();
        let geometry = popover_geometry(area, panel, 10, &body).unwrap();
        assert_eq!(geometry.rectangle.x, panel.right() + 1);
        assert_eq!(geometry.rectangle.y, 10, "level with the row");
        assert!(geometry.rectangle.width <= MAXIMUM_WIDTH);
    }

    #[test]
    fn multiple_disabled_options_are_listed_with_their_labels() {
        let options = vec![
            DisabledOption {
                label: "GPU".to_owned(),
                reason: "No driver.".to_owned(),
            },
            DisabledOption {
                label: "Other".to_owned(),
                reason: "Not here.".to_owned(),
            },
        ];
        let (title, body) = popover_text(&options);
        assert_eq!(title.trim(), "Unavailable options");
        assert_eq!(body, "GPU: No driver.\n\nOther: Not here.");
        assert_eq!(popover_text(&[]), (String::new(), String::new()));
    }
}
