//! Shared geometry for project animation controls and the live Braille preview.
use crate::{
    app::{App, SettingsState},
    background_animation::{AnimationKind, DitherMode},
};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    Frame,
};

pub const ROW_COUNT: usize = 14;
/// ASCII label padding shared by the rendered chevrons and their hit zones.
const STEPPER_LABEL_WIDTH: u16 = 18;

#[derive(Debug, Clone, Copy)]
pub struct AnimationLayout {
    pub controls: Rect,
    pub preview: Rect,
}

pub fn layout(area: Rect) -> AnimationLayout {
    if area.width >= 72 {
        let width = 40.min(area.width / 2);
        AnimationLayout {
            controls: Rect::new(area.x, area.y, width, area.height),
            preview: Rect::new(
                area.x + width + 2,
                area.y,
                area.width.saturating_sub(width + 2),
                area.height,
            ),
        }
    } else {
        let height = (area.height.saturating_sub(2) / 2)
            .clamp(3, 17)
            .min(area.height);
        AnimationLayout {
            controls: Rect::new(area.x, area.y, area.width, height),
            preview: Rect::new(
                area.x,
                area.y + height,
                area.width,
                area.height.saturating_sub(height),
            ),
        }
    }
}

pub fn max_scroll(area: Rect) -> u16 {
    (ROW_COUNT as u16).saturating_sub(layout(area).controls.height.saturating_sub(3))
}

pub fn scroll_for_selection(area: Rect, row: usize, scroll: u16) -> u16 {
    let visible = layout(area).controls.height.saturating_sub(3).max(1);
    let row = row as u16;
    let scroll = if row < scroll {
        row
    } else if row >= scroll + visible {
        row + 1 - visible
    } else {
        scroll
    };
    scroll.min(max_scroll(area))
}

pub fn row_y(area: Rect, row: usize, scroll: u16) -> Option<u16> {
    let controls = layout(area).controls;
    if controls.width == 0 || controls.height <= 3 || row >= ROW_COUNT {
        return None;
    }
    let relative = u16::try_from(row).ok()?.checked_sub(scroll)?;
    let y = controls.y.checked_add(2)?.checked_add(relative)?;
    (controls.contains(Position::new(controls.x, y)) && y < controls.bottom().saturating_sub(1))
        .then_some(y)
}

/// Scene labels select directly. Only the actual left-chevron hot zone decrements.
pub fn hit(area: Rect, scroll: u16, position: Position) -> Option<(usize, i32)> {
    let controls = layout(area).controls;
    if !controls.contains(position) {
        return None;
    }
    let relative = position.y.checked_sub(controls.y + 2)?;
    let row = usize::from(relative + scroll);
    if row >= ROW_COUNT || row_y(area, row, scroll) != Some(position.y) {
        return None;
    }
    Some((
        row,
        if position.x.saturating_sub(controls.x) == STEPPER_LABEL_WIDTH {
            -1
        } else {
            1
        },
    ))
}

fn stepper_text(label: &str, value: impl std::fmt::Display) -> String {
    format!(
        "{label:<width$}‹ {value} ›",
        width = usize::from(STEPPER_LABEL_WIDTH)
    )
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let geometry = layout(area);
    // Light mode follows the terminal-owned light canvas; dark mode keeps
    // the original black monochrome canvas. Dots and controls share the ink.
    let ink = match app.ui_settings.color_scheme {
        crate::theme::ColorScheme::Light => Style::default().fg(Color::Black).bg(Color::Reset),
        crate::theme::ColorScheme::Dark => Style::default().fg(Color::White).bg(Color::Black),
    };
    frame.render_widget(
        Paragraph::new("Scenes — select to preview").style(ink.add_modifier(Modifier::BOLD)),
        Rect::new(
            geometry.controls.x,
            geometry.controls.y,
            geometry.controls.width,
            1,
        ),
    );
    for row in 0..ROW_COUNT {
        let Some(y) = row_y(area, row, state.scroll) else {
            continue;
        };
        let text = if let Some(kind) = AnimationKind::ALL.get(row) {
            format!(
                "{} {:2}. {}",
                if *kind == app.animation_settings.kind {
                    "●"
                } else {
                    " "
                },
                row + 1,
                kind.label()
            )
        } else {
            match row {
                10 => stepper_text(
                    "Background",
                    if app.animation_settings.enabled {
                        "On"
                    } else {
                        "Off"
                    },
                ),
                11 => stepper_text(
                    "Speed",
                    format_args!("{}%", app.animation_settings.speed_percent),
                ),
                12 => stepper_text(
                    "Density",
                    format_args!("{}%", app.animation_settings.density_percent),
                ),
                13 => stepper_text(
                    "Dither",
                    match app.animation_settings.dither {
                        DitherMode::Ordered => "Ordered",
                        DitherMode::Stippled => "Stippled",
                    },
                ),
                _ => continue,
            }
        };
        let style = if row == state.selected_row {
            ink.add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            ink
        };
        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(geometry.controls.x, y, geometry.controls.width, 1),
        );
    }
    let hint = "↑↓ select · ←→ adjust · ? help";
    if geometry.controls.height > 1 {
        frame.render_widget(
            Paragraph::new(hint).style(ink),
            Rect::new(
                geometry.controls.x,
                geometry.controls.bottom() - 1,
                geometry.controls.width,
                1,
            ),
        );
    }
    let preview = geometry.preview;
    if preview.width == 0 || preview.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("Live preview").style(ink.add_modifier(Modifier::BOLD)),
        Rect::new(preview.x, preview.y, preview.width, 1),
    );
    let raster = Rect::new(
        preview.x,
        preview.y + 1,
        preview.width,
        preview.height.saturating_sub(4),
    );
    let mut animation = app.animation_preview_frame.borrow_mut();
    let elapsed = crate::background_composition::quantized_elapsed(app.started_at.elapsed());
    animation.render(
        &app.animation_settings,
        raster.width,
        raster.height,
        elapsed,
    );
    for y in 0..raster.height {
        for x in 0..raster.width {
            frame.buffer_mut()[(raster.x + x, raster.y + y)]
                .set_char(animation.glyph(x, y))
                .set_style(ink);
        }
    }
    if preview.height >= 4 {
        frame.render_widget(
            Paragraph::new(app.animation_settings.kind.description()).style(ink),
            Rect::new(preview.x, preview.bottom() - 3, preview.width, 1),
        );
    }
    if preview.height >= 3 {
        frame.render_widget(
            Paragraph::new("Demo ignores Background / Motion Off").style(ink),
            Rect::new(preview.x, preview.bottom() - 2, preview.width, 1),
        );
        let message = app
            .status_message
            .as_deref()
            .unwrap_or("Saved automatically for this project.");
        frame.render_widget(
            Paragraph::new(message).style(ink),
            Rect::new(preview.x, preview.bottom() - 1, preview.width, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_settings_render_keeps_every_selection_and_a_useful_preview_at_standard_sizes() {
        use crate::app::SettingsTab;
        use ratatui::{backend::TestBackend, Terminal};
        for (width, height) in [(80, 24), (100, 30), (140, 40)] {
            let project = tempfile::tempdir().unwrap();
            let app = App::new("test".into(), project.path().to_path_buf());
            let area =
                crate::settings_ui::compute_layout(Rect::new(0, 0, width, height)).content_area;
            let geometry = layout(area);
            assert!(
                geometry.preview.height.saturating_sub(4) >= 7,
                "{width}×{height} preview too small"
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for row in 0..ROW_COUNT {
                let scroll = scroll_for_selection(area, row, 0);
                let state = SettingsState {
                    tab: SettingsTab::Animations,
                    selected_row: row,
                    scroll,
                    ..Default::default()
                };
                let y = row_y(area, row, scroll).expect("selected row remains visible");
                terminal
                    .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
                    .unwrap();
                let cell = &terminal.backend().buffer()[(geometry.controls.x, y)];
                assert!(cell.modifier.contains(Modifier::REVERSED));
                let dots = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .filter(|cell| {
                        cell.symbol()
                            .chars()
                            .any(|glyph| ('\u{2801}'..='\u{28ff}').contains(&glyph))
                    })
                    .count();
                assert!(dots > 0, "{width}×{height} preview must contain the scene");
            }
        }
    }

    #[test]
    fn preview_dots_and_controls_contrast_with_both_color_schemes() {
        use crate::{app::SettingsTab, theme::ColorScheme};
        use ratatui::{backend::TestBackend, Terminal};
        for (scheme, foreground, background) in [
            (ColorScheme::Dark, Color::White, Color::Black),
            (ColorScheme::Light, Color::Black, Color::Reset),
        ] {
            let project = tempfile::tempdir().unwrap();
            let mut app = App::new("test".into(), project.path().to_path_buf());
            app.ui_settings.color_scheme = scheme;
            let state = SettingsState {
                tab: SettingsTab::Animations,
                selected_row: 11,
                ..Default::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            terminal
                .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
                .unwrap();
            let area = crate::settings_ui::compute_layout(Rect::new(0, 0, 140, 40)).content_area;
            let geometry = layout(area);
            let controls_y = row_y(area, 10, 0).unwrap();
            let control = &terminal.backend().buffer()[(geometry.controls.x, controls_y)];
            assert_eq!(control.fg, foreground);
            assert_eq!(control.bg, background);
            let mut dots = 0;
            for y in geometry.preview.y + 1..geometry.preview.bottom().saturating_sub(3) {
                for x in geometry.preview.x..geometry.preview.right() {
                    let cell = &terminal.backend().buffer()[(x, y)];
                    if cell
                        .symbol()
                        .chars()
                        .any(|glyph| ('\u{2801}'..='\u{28ff}').contains(&glyph))
                    {
                        dots += 1;
                        assert_eq!(cell.fg, foreground, "visible dot ink follows {scheme:?}");
                        assert_eq!(cell.bg, background, "dot canvas follows {scheme:?}");
                        assert_ne!(cell.fg, cell.bg);
                    }
                }
            }
            assert!(dots > 0, "contrast assertion must cover real rendered dots");
        }
    }

    #[test]
    fn rendered_preview_advances_with_background_disabled_and_motion_off() {
        use crate::app::SettingsTab;
        use ratatui::{backend::TestBackend, Terminal};
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.ui_settings.motion_level = crate::config::MotionLevel::Off;
        let state = SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
            .unwrap();
        let first = terminal.backend().buffer().clone();
        app.started_at = std::time::Instant::now() - std::time::Duration::from_secs(5);
        terminal
            .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
            .unwrap();
        assert_ne!(&first, terminal.backend().buffer());
        assert!(!app.animation_settings.enabled);
    }

    #[test]
    fn every_scene_is_reachable_by_keyboard_and_immediately_rendered() {
        use crate::app::{Mode, SettingsTab};
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        use ratatui::{backend::TestBackend, Terminal};
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 80, 24);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut prior = None;
        for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
            if index > 0 {
                crate::keys::handle_event(
                    &mut app,
                    Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
                );
            }
            assert_eq!(app.animation_settings.kind, kind);
            let Mode::Settings(state) = &app.mode else {
                panic!("selection must stay in Settings")
            };
            assert_eq!(state.selected_row, index);
            terminal
                .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, state))
                .unwrap();
            let geometry =
                layout(crate::settings_ui::compute_layout(app.layout.screen_area).content_area);
            let raster = (0..geometry.preview.height.saturating_sub(4))
                .flat_map(|y| {
                    let buffer = terminal.backend().buffer();
                    (0..geometry.preview.width).map(move |x| {
                        buffer[(geometry.preview.x + x, geometry.preview.y + 1 + y)]
                            .symbol()
                            .to_owned()
                    })
                })
                .collect::<Vec<_>>();
            if let Some(prior) = prior.as_ref() {
                assert_ne!(prior, &raster, "scene preview changes immediately");
            }
            prior = Some(raster);
        }
    }

    #[test]
    fn keyboard_and_mouse_select_the_same_persisted_scene() {
        use crate::app::{Mode, SettingsTab};
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 140, 40);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        );
        assert_eq!(app.animation_settings.kind, AnimationKind::MoonlitWater);
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        let y = row_y(area, 5, 0).unwrap();
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation
                .kind,
            AnimationKind::Kelp
        );
    }

    #[test]
    fn live_preview_is_explicit_and_covering_help_stops_it() {
        use crate::app::{Mode, SettingsTab};
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.ui_settings.motion_level = crate::config::MotionLevel::Off;
        assert!(!app.is_animation_preview_visible());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        assert!(app.is_animation_preview_visible());
        let parent = std::mem::replace(&mut app.mode, Mode::Normal);
        app.push_modal_over(
            parent,
            Mode::SettingsHelp(crate::settings_help::dialog::SettingsHelpState::new(
                "AN-01",
                1,
                crate::config::MotionLevel::Off,
            )),
        );
        assert!(!app.is_animation_preview_visible());
    }

    #[test]
    fn adjustment_controls_obey_bounds_and_persist_with_the_selected_scene() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.settings_adjust_animation_row(9, 1);
        for _ in 0..10 {
            app.settings_adjust_animation_row(11, -1);
            app.settings_adjust_animation_row(12, -1);
        }
        assert_eq!(
            (
                app.animation_settings.speed_percent,
                app.animation_settings.density_percent
            ),
            (25, 25)
        );
        for _ in 0..40 {
            app.settings_adjust_animation_row(11, 1);
            app.settings_adjust_animation_row(12, 1);
        }
        assert_eq!(
            (
                app.animation_settings.speed_percent,
                app.animation_settings.density_percent
            ),
            (200, 100)
        );
        app.settings_adjust_animation_row(10, 1);
        app.settings_adjust_animation_row(13, 1);
        assert!(app.animation_settings.enabled);
        assert_eq!(app.animation_settings.dither, DitherMode::Stippled);
        assert_eq!(
            app.animation_settings.kind,
            AnimationKind::BreathingMountain
        );
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
    }

    #[test]
    fn actual_mouse_clicks_on_rendered_chevrons_adjust_in_the_displayed_direction() {
        use crate::app::{Mode, SettingsTab};
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{backend::TestBackend, Terminal};
        for (width, height) in [(80, 24), (100, 30)] {
            let project = tempfile::tempdir().unwrap();
            let mut app = App::new("test".into(), project.path().to_path_buf());
            app.layout.screen_area = Rect::new(0, 0, width, height);
            let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for row in [11, 12] {
                let scroll = scroll_for_selection(area, row, 0);
                app.mode = Mode::Settings(SettingsState {
                    tab: SettingsTab::Animations,
                    selected_row: row,
                    scroll,
                    ..Default::default()
                });
                let initial = if row == 11 {
                    app.animation_settings.speed_percent
                } else {
                    app.animation_settings.density_percent
                };
                for (symbol, expected) in [
                    ("‹", initial - if row == 11 { 25 } else { 5 }),
                    ("›", initial),
                ] {
                    let Mode::Settings(state) = &app.mode else {
                        panic!("Settings remains open")
                    };
                    terminal
                        .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, state))
                        .unwrap();
                    let y = row_y(area, row, state.scroll).unwrap();
                    let controls = layout(area).controls;
                    let x = (controls.x..controls.right())
                        .find(|x| terminal.backend().buffer()[(*x, y)].symbol() == symbol)
                        .expect("rendered chevron exists");
                    crate::mouse::handle_mouse_event(
                        &mut app,
                        MouseEvent {
                            kind: MouseEventKind::Down(MouseButton::Left),
                            column: x,
                            row: y,
                            modifiers: KeyModifiers::NONE,
                        },
                    );
                    let value = if row == 11 {
                        app.animation_settings.speed_percent
                    } else {
                        app.animation_settings.density_percent
                    };
                    assert_eq!(value, expected, "{width}×{height} row{row} clicked{symbol}");
                }
            }
        }
    }

    #[test]
    fn row_y_never_escapes_an_empty_or_short_controls_rectangle() {
        for width in 0..4 {
            for height in 0..8 {
                let area = Rect::new(3, 4, width, height);
                for row in 0..ROW_COUNT {
                    if let Some(y) = row_y(area, row, scroll_for_selection(area, row, 0)) {
                        assert!(layout(area).controls.contains(Position::new(area.x, y)));
                    }
                }
            }
        }
    }

    #[test]
    fn scene_and_control_hits_follow_scrolled_geometry_at_all_sizes() {
        for (width, height) in [(100, 30), (60, 24), (20, 8), (5, 3), (0, 0)] {
            let area = Rect::new(3, 4, width, height);
            for row in 0..ROW_COUNT {
                let scroll = scroll_for_selection(area, row, 0);
                if let Some(y) = row_y(area, row, scroll) {
                    assert_eq!(
                        hit(area, scroll, Position::new(area.x, y)).map(|(row, _)| row),
                        Some(row)
                    );
                }
            }
        }
    }
    #[test]
    fn selection_persists_while_background_is_disabled_and_failure_retains_effective_scene() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.settings_adjust_animation_row(5, 1);
        assert!(!app.animation_settings.enabled);
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
        let prior = app.animation_settings;
        std::fs::remove_file(project.path().join(".ilium/config.yaml")).unwrap();
        std::fs::create_dir(project.path().join(".ilium/config.yaml")).unwrap();
        app.settings_adjust_animation_row(1, 1);
        assert_eq!(app.animation_settings, prior);
        assert!(app
            .status_message
            .as_deref()
            .unwrap()
            .contains("Could not save"));
    }
}
