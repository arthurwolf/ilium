//! Shared geometry for real sliders, project controls and the live dot preview.

use crate::{
    app::{App, SettingsState},
    background_animation::{AnimationKind, AnimationSettings, DitherMode},
};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    Frame,
};

pub const ROW_COUNT: usize = 21;

#[derive(Debug, Clone, Copy)]
pub struct AnimationLayout {
    pub controls: Rect,
    pub preview: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct SliderGeometry {
    pub label: Rect,
    pub track: Rect,
    pub value: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationHit {
    Select(usize),
    Toggle(usize),
    Slider { row: usize, value: u16 },
    ScrollTo(u16),
}

pub fn layout(area: Rect) -> AnimationLayout {
    if area.width >= 72 {
        let width = 44.min(area.width / 2);
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

fn visible_rows(area: Rect) -> u16 {
    layout(area).controls.height.saturating_sub(3)
}

pub fn max_scroll(area: Rect) -> u16 {
    (ROW_COUNT as u16).saturating_sub(visible_rows(area))
}

pub fn scroll_for_selection(area: Rect, row: usize, scroll: u16) -> u16 {
    let visible = visible_rows(area).max(1);
    let row = row.min(ROW_COUNT - 1) as u16;
    let scroll = if row < scroll {
        row
    } else if row >= scroll.saturating_add(visible) {
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
    (y < controls.bottom().saturating_sub(1)).then_some(y)
}

fn is_slider_row(row: usize) -> bool {
    matches!(row, 11 | 12 | 14..=20)
}

/// Labels and value readouts have their own rectangles. Only the track changes
/// a numeric value; clicking its label focuses the row without jumping it.
pub fn slider_geometry(area: Rect, row: usize, scroll: u16) -> Option<SliderGeometry> {
    if !is_slider_row(row) {
        return None;
    }
    let y = row_y(area, row, scroll)?;
    let controls = layout(area).controls;
    let usable = controls.width.saturating_sub(1);
    if usable < 12 {
        return None;
    }
    let value_width = 5;
    let label_width = if usable >= 36 {
        18
    } else {
        usable.saturating_sub(10).min(14)
    };
    let track_width = usable.saturating_sub(label_width + value_width + 2);
    if track_width < 2 {
        return None;
    }
    Some(SliderGeometry {
        label: Rect::new(controls.x, y, label_width, 1),
        track: Rect::new(controls.x + label_width + 1, y, track_width, 1),
        value: Rect::new(controls.right() - 1 - value_width, y, value_width, 1),
    })
}

/// Dragging uses the owned row, independent of the pointer's vertical position.
/// Horizontal motion beyond either endpoint clamps to that endpoint.
pub fn slider_value_at(
    area: Rect,
    row: usize,
    scroll: u16,
    column: u16,
    settings: &AnimationSettings,
) -> Option<u16> {
    let geometry = slider_geometry(area, row, scroll)?;
    let slider = settings.slider(row)?;
    Some(slider.value_at(
        column.saturating_sub(geometry.track.x),
        geometry.track.width,
    ))
}

pub fn hit(
    area: Rect,
    scroll: u16,
    position: Position,
    settings: &AnimationSettings,
) -> Option<AnimationHit> {
    let controls = layout(area).controls;
    if !controls.contains(position) {
        return None;
    }
    let relative = position.y.checked_sub(controls.y + 2)?;
    let row = usize::from(relative + scroll);
    if row >= ROW_COUNT || row_y(area, row, scroll) != Some(position.y) {
        return None;
    }
    let visible = visible_rows(area);
    if position.x == controls.right().saturating_sub(1) && max_scroll(area) > 0 {
        let target = u32::from(max_scroll(area)) * u32::from(relative)
            / u32::from(visible.saturating_sub(1).max(1));
        return Some(AnimationHit::ScrollTo(target as u16));
    }
    if let Some(geometry) = slider_geometry(area, row, scroll) {
        if geometry.track.contains(position) {
            return slider_value_at(area, row, scroll, position.x, settings)
                .map(|value| AnimationHit::Slider { row, value });
        }
    }
    Some(if matches!(row, 10 | 13) {
        AnimationHit::Toggle(row)
    } else {
        AnimationHit::Select(row)
    })
}

fn control_ink(app: &App) -> Style {
    match app.ui_settings.color_scheme {
        crate::theme::ColorScheme::Light => Style::default().fg(Color::Black).bg(Color::Reset),
        crate::theme::ColorScheme::Dark => Style::default().fg(Color::White).bg(Color::Black),
    }
}

fn draw_slider(
    frame: &mut Frame,
    area: Rect,
    row: usize,
    state: &SettingsState,
    settings: &AnimationSettings,
    style: Style,
) {
    let Some(slider) = settings.slider(row) else {
        return;
    };
    let Some(y) = row_y(area, row, state.scroll) else {
        return;
    };
    let Some(geometry) = slider_geometry(area, row, state.scroll) else {
        let controls = layout(area).controls;
        frame.render_widget(
            Paragraph::new(format!("{} {}{}", slider.label, slider.value, slider.unit))
                .style(style),
            Rect::new(controls.x, y, controls.width.saturating_sub(1), 1),
        );
        return;
    };
    frame.render_widget(Paragraph::new(slider.label).style(style), geometry.label);
    frame.render_widget(
        Paragraph::new(format!("{:>3}{}", slider.value, slider.unit)).style(style),
        geometry.value,
    );
    let thumb = slider.thumb_offset(geometry.track.width);
    for offset in 0..geometry.track.width {
        let symbol = if offset == thumb { '●' } else { '─' };
        frame.buffer_mut()[(geometry.track.x + offset, y)]
            .set_char(symbol)
            .set_style(style);
    }
}

fn draw_scrollbar(frame: &mut Frame, area: Rect, scroll: u16, ink: Style) {
    let controls = layout(area).controls;
    let visible = visible_rows(area);
    let maximum = max_scroll(area);
    if visible == 0 || maximum == 0 || controls.width == 0 {
        return;
    }
    let thumb_size = (u32::from(visible) * u32::from(visible) / ROW_COUNT as u32).max(1) as u16;
    let travel = visible.saturating_sub(thumb_size);
    let thumb_start =
        (u32::from(scroll.min(maximum)) * u32::from(travel) / u32::from(maximum)) as u16;
    for offset in 0..visible {
        let character = if (thumb_start..thumb_start + thumb_size).contains(&offset) {
            '┃'
        } else {
            '│'
        };
        frame.buffer_mut()[(controls.right() - 1, controls.y + 2 + offset)]
            .set_char(character)
            .set_style(ink.add_modifier(Modifier::DIM));
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, state: &SettingsState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let geometry = layout(area);
    let ink = control_ink(app);
    let settings = app.animation_settings.normalized();
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
        let style = if row == state.selected_row {
            ink.add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            ink
        };
        let row_area = Rect::new(
            geometry.controls.x,
            y,
            geometry.controls.width.saturating_sub(1),
            1,
        );
        frame.render_widget(
            Paragraph::new(" ".repeat(usize::from(row_area.width))).style(style),
            row_area,
        );
        if let Some(kind) = AnimationKind::ALL.get(row) {
            frame.render_widget(
                Paragraph::new(format!(
                    "{} {:2}. {}",
                    if *kind == settings.kind { "●" } else { " " },
                    row + 1,
                    kind.label()
                ))
                .style(style),
                row_area,
            );
        } else if is_slider_row(row) {
            draw_slider(frame, area, row, state, &settings, style);
        } else {
            let text = match row {
                10 => format!(
                    "Background        [ {} ]",
                    if settings.enabled { "On" } else { "Off" }
                ),
                13 => format!(
                    "Dither            [ {} ]",
                    match settings.dither {
                        DitherMode::Ordered => "Ordered",
                        DitherMode::Stippled => "Stippled",
                    }
                ),
                _ => continue,
            };
            frame.render_widget(Paragraph::new(text).style(style), row_area);
        }
    }
    draw_scrollbar(frame, area, state.scroll, ink);
    if geometry.controls.height > 1 {
        frame.render_widget(
            Paragraph::new("↑↓ select · Enter sliders · ←→ adjust").style(ink),
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
    let (red, green, blue) = settings.foreground_rgb();
    let dot_ink = Style::default().fg(Color::Rgb(red, green, blue)).bg(
        if app.ui_settings.color_scheme == crate::theme::ColorScheme::Dark {
            Color::Black
        } else {
            Color::Reset
        },
    );
    let mut animation = app.animation_preview_frame.borrow_mut();
    let elapsed = crate::background_composition::quantized_elapsed(app.started_at.elapsed());
    animation.render(&settings, raster.width, raster.height, elapsed);
    for y in 0..raster.height {
        for x in 0..raster.width {
            frame.buffer_mut()[(raster.x + x, raster.y + y)]
                .set_char(animation.glyph(x, y))
                .set_style(dot_ink);
        }
    }
    if preview.height >= 4 {
        let description = settings.slider(state.selected_row).map_or_else(
            || settings.kind.description().to_owned(),
            |slider| {
                format!(
                    "{}: {}..{}{}",
                    slider.label, slider.minimum, slider.maximum, slider.unit
                )
            },
        );
        frame.render_widget(
            Paragraph::new(description).style(ink),
            Rect::new(preview.x, preview.bottom() - 3, preview.width, 1),
        );
    }
    if preview.height >= 3 {
        frame.render_widget(
            Paragraph::new("Demo ignores Background / Motion Off").style(ink),
            Rect::new(preview.x, preview.bottom() - 2, preview.width, 1),
        );
        frame.render_widget(
            Paragraph::new(
                app.status_message
                    .as_deref()
                    .unwrap_or("Saved automatically for this project."),
            )
            .style(ink),
            Rect::new(preview.x, preview.bottom() - 1, preview.width, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Mode, SettingsTab};
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn key(app: &mut App, code: KeyCode) {
        crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
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

    fn rendered_dots(terminal: &Terminal<TestBackend>, rectangle: Rect) -> Vec<(String, Color)> {
        (rectangle.top()..rectangle.bottom())
            .flat_map(|y| (rectangle.left()..rectangle.right()).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                let cell = &terminal.backend().buffer()[(x, y)];
                cell.symbol()
                    .chars()
                    .any(|glyph| ('\u{2801}'..='\u{28ff}').contains(&glyph))
                    .then(|| (cell.symbol().to_owned(), cell.fg))
            })
            .collect()
    }

    #[test]
    fn real_render_keeps_every_row_visible_with_a_useful_preview_at_standard_sizes() {
        for (width, height) in [(80, 24), (100, 30), (140, 40)] {
            let project = tempfile::tempdir().unwrap();
            let app = App::new("test".into(), project.path().to_path_buf());
            let area =
                crate::settings_ui::compute_layout(Rect::new(0, 0, width, height)).content_area;
            let geometry = layout(area);
            assert!(geometry.preview.height.saturating_sub(4) >= 7);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for row in 0..ROW_COUNT {
                let state = SettingsState {
                    tab: SettingsTab::Animations,
                    selected_row: row,
                    scroll: scroll_for_selection(area, row, 0),
                    ..Default::default()
                };
                let y = row_y(area, row, state.scroll).unwrap();
                terminal
                    .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
                    .unwrap();
                assert!(terminal.backend().buffer()[(geometry.controls.x, y)]
                    .modifier
                    .contains(Modifier::REVERSED));
                assert!(!rendered_dots(&terminal, geometry.preview).is_empty());
                if let Some(track) = slider_geometry(area, row, state.scroll) {
                    assert!(track.track.width >= 2);
                    assert!((track.track.x..track.track.right()).any(|x| terminal
                        .backend()
                        .buffer()[(x, y)]
                        .symbol()
                        == "●"));
                }
            }
        }
    }

    #[test]
    fn preview_palette_matches_the_shared_hsl_for_both_canvases() {
        for scheme in [
            crate::theme::ColorScheme::Dark,
            crate::theme::ColorScheme::Light,
        ] {
            let project = tempfile::tempdir().unwrap();
            let mut app = App::new("test".into(), project.path().to_path_buf());
            app.ui_settings.color_scheme = scheme;
            let state = SettingsState {
                tab: SettingsTab::Animations,
                ..Default::default()
            };
            let area = crate::settings_ui::compute_layout(Rect::new(0, 0, 140, 40)).content_area;
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            for (lightness, hue, saturation, expected) in [
                (60, 210, 0, Color::Rgb(153, 153, 153)),
                (50, 120, 100, Color::Rgb(0, 255, 0)),
            ] {
                app.animation_settings.lightness_percent = lightness;
                app.animation_settings.hue_degrees = hue;
                app.animation_settings.saturation_percent = saturation;
                terminal
                    .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
                    .unwrap();
                let dots = rendered_dots(&terminal, layout(area).preview);
                assert!(!dots.is_empty());
                assert!(dots.iter().all(|(_, ink)| *ink == expected));
            }
        }
    }

    #[test]
    fn keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 80, 24);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        for row in 0..ROW_COUNT {
            if row > 0 {
                key(&mut app, KeyCode::Down);
            }
            if row < 10 {
                assert_eq!(app.animation_settings.kind, AnimationKind::ALL[row]);
            }
            let Mode::Settings(state) = &app.mode else {
                panic!("Settings stays open");
            };
            assert_eq!(state.selected_row, row);
            assert!(
                row_y(area, row, state.scroll).is_some(),
                "row{row} remains visible"
            );
        }
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert!(state.scroll > 0);
        let before = app.animation_settings.quiet_pond.drift_percent;
        key(&mut app, KeyCode::Right);
        assert!(app.animation_settings.quiet_pond.drift_percent > before);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
    }

    #[test]
    fn keyboard_and_mouse_select_the_same_persisted_scene() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 140, 40);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        key(&mut app, KeyCode::Down);
        assert_eq!(app.animation_settings.kind, AnimationKind::MoonlitWater);
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            area.x,
            row_y(area, 5, 0).unwrap(),
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
    fn actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus() {
        for (width, height) in [(80, 24), (140, 40)] {
            let project = tempfile::tempdir().unwrap();
            let mut app = App::new("test".into(), project.path().to_path_buf());
            app.layout.screen_area = Rect::new(0, 0, width, height);
            let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for row in [11, 12, 14, 15, 16, 17, 18, 19, 20] {
                let scroll = scroll_for_selection(area, row, 0);
                app.mode = Mode::Settings(SettingsState {
                    tab: SettingsTab::Animations,
                    selected_row: row,
                    scroll,
                    ..Default::default()
                });
                let geometry = slider_geometry(area, row, scroll).unwrap();
                let slider = app.animation_settings.slider(row).unwrap();
                let Mode::Settings(state) = &app.mode else {
                    panic!("Settings stays open");
                };
                terminal
                    .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, state))
                    .unwrap();
                assert!((geometry.track.x..geometry.track.right()).any(|x| terminal
                    .backend()
                    .buffer()[(x, geometry.track.y)]
                    .symbol()
                    == "●"));
                pointer(
                    &mut app,
                    MouseEventKind::Down(MouseButton::Left),
                    geometry.label.x,
                    geometry.label.y,
                );
                assert_eq!(
                    app.animation_settings.slider(row).unwrap().value,
                    slider.value,
                    "label must not jump"
                );
                for (column, expected) in [
                    (geometry.track.x, slider.minimum),
                    (geometry.track.right() - 1, slider.maximum),
                ] {
                    pointer(
                        &mut app,
                        MouseEventKind::Down(MouseButton::Left),
                        column,
                        geometry.track.y,
                    );
                    assert_eq!(app.animation_settings.slider(row).unwrap().value, expected);
                    pointer(
                        &mut app,
                        MouseEventKind::Up(MouseButton::Left),
                        column,
                        geometry.track.y,
                    );
                    assert_eq!(
                        crate::project_config::load(project.path())
                            .unwrap()
                            .animation,
                        app.animation_settings
                    );
                }
            }
        }
    }

    #[test]
    fn owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.set_screen_area(Rect::new(0, 0, 140, 40));
        let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        let scroll = scroll_for_selection(area, 14, 0);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            selected_row: 14,
            scroll,
            ..Default::default()
        });
        let geometry = slider_geometry(area, 14, scroll).unwrap();
        let voice_enabled = app.voice_settings.enabled;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            geometry.track.x,
            geometry.track.y,
        );
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 139, 39);
        assert_eq!(app.animation_settings.lightness_percent, 100);
        let voice_area = app.layout.voice_control_area;
        assert!(voice_area.width > 0 && voice_area.height > 0);
        pointer(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            voice_area.x,
            voice_area.y,
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("owned release stays in Settings");
        };
        assert_eq!(state.animation_slider_drag, None);
        assert_eq!(app.voice_settings.enabled, voice_enabled);
        pointer(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            geometry.track.x,
            geometry.track.y,
        );
        assert_eq!(
            app.animation_settings.lightness_percent, 100,
            "unowned drag is inert"
        );
    }

    #[test]
    fn palette_and_named_controls_survive_scene_switches_and_failed_saves() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.settings_set_animation_slider(14, 35);
        app.settings_set_animation_slider(15, 125);
        app.settings_set_animation_slider(16, 50);
        app.settings_adjust_animation_row(5, 1);
        app.settings_set_animation_slider(17, 175);
        app.settings_adjust_animation_row(1, 1);
        app.settings_set_animation_slider(17, 35);
        app.settings_adjust_animation_row(5, 1);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 175);
        assert_eq!(
            app.animation_settings.moonlit_water.wave_strength_percent,
            35
        );
        assert_eq!(
            (
                app.animation_settings.lightness_percent,
                app.animation_settings.hue_degrees,
                app.animation_settings.saturation_percent
            ),
            (35, 125, 50)
        );
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation,
            app.animation_settings
        );
        let before = app.animation_settings;
        std::fs::remove_file(project.path().join(".ilium/config.yaml")).unwrap();
        std::fs::create_dir(project.path().join(".ilium/config.yaml")).unwrap();
        app.settings_set_animation_slider(14, 80);
        assert_eq!(app.animation_settings, before);
        assert!(app
            .status_message
            .as_deref()
            .unwrap()
            .contains("Could not save"));
    }

    #[test]
    fn actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches() {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.layout.screen_area = Rect::new(0, 0, 80, 24);
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        for _ in 0..5 {
            key(&mut app, KeyCode::Down);
        }
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        key(&mut app, KeyCode::Enter);
        let Mode::Settings(state) = &app.mode else {
            panic!("Settings stays open");
        };
        assert_eq!(state.selected_row, 17, "Enter focuses scene controls");
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        key(&mut app, KeyCode::Right);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 105);
        for _ in 0..3 {
            key(&mut app, KeyCode::Up);
        }
        key(&mut app, KeyCode::Right);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Right);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Right);
        assert_eq!(
            (
                app.animation_settings.lightness_percent,
                app.animation_settings.hue_degrees,
                app.animation_settings.saturation_percent
            ),
            (61, 211, 1)
        );
        for _ in 0..15 {
            key(&mut app, KeyCode::Up);
        }
        assert_eq!(app.animation_settings.kind, AnimationKind::MoonlitWater);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Right);
        assert_eq!(
            app.animation_settings.moonlit_water.wave_strength_percent,
            105
        );
        for _ in 0..12 {
            key(&mut app, KeyCode::Up);
        }
        assert_eq!(app.animation_settings.kind, AnimationKind::Kelp);
        assert_eq!(app.animation_settings.kelp.plant_density_percent, 105);
        let reloaded = crate::project_config::load(project.path())
            .unwrap()
            .animation;
        assert_eq!(reloaded, app.animation_settings);
        assert_eq!(
            (
                reloaded.lightness_percent,
                reloaded.hue_degrees,
                reloaded.saturation_percent
            ),
            (61, 211, 1)
        );
        assert_eq!(reloaded.moonlit_water.wave_strength_percent, 105);
    }

    #[test]
    fn preview_advances_while_disabled_or_off_and_help_covers_it() {
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
        assert_ne!(first, *terminal.backend().buffer());
        assert!(!app.animation_settings.enabled);
        app.mode = Mode::Settings(state);
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
    fn row_and_hit_geometry_never_escape_short_or_scrolled_controls() {
        let settings = AnimationSettings::default();
        for (width, height) in [(100, 30), (60, 24), (20, 8), (5, 3), (0, 0)] {
            let area = Rect::new(3, 4, width, height);
            for row in 0..ROW_COUNT {
                let scroll = scroll_for_selection(area, row, 0);
                if let Some(y) = row_y(area, row, scroll) {
                    assert!(layout(area).controls.contains(Position::new(area.x, y)));
                    let expected = if matches!(row, 10 | 13) {
                        AnimationHit::Toggle(row)
                    } else {
                        AnimationHit::Select(row)
                    };
                    assert_eq!(
                        hit(area, scroll, Position::new(area.x, y), &settings),
                        Some(expected)
                    );
                }
            }
        }
    }
}
