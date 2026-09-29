//! Opt-in render receipts for the complete catalog, produced by the real UI.

use crate::app::{App, Mode, SettingsState, SettingsTab};
use crate::background_animation::{AnimationKind, AnimationSettings};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};
use std::time::{Duration, Instant};

#[test]
fn ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background() {
    use crate::app::{PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use ilium_core::{PaneContentKind, ROOT_ID};
    let project = tempfile::tempdir().unwrap();
    let mut app = App::new("test".into(), project.path().to_path_buf());
    let group = app.tree.add_group(ROOT_ID, "work").unwrap();
    let pane_id = app
        .tree
        .add_pane(group, "shell", PaneContentKind::Terminal)
        .unwrap();
    let mut terminal_view = TerminalView::new(40, 90);
    terminal_view.feed(b"REAL CONTENT\r\n\x1b[41m    \x1b[0m");
    let original = terminal_view.with_screen(vt100::Screen::contents);
    app.panes
        .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal_view)));
    app.right_panel_target = RightPanelTarget::Pane { pane_id };
    app.set_screen_area(Rect::new(0, 0, 140, 40));
    app.animation_settings.enabled = true;
    app.animation_settings.kind = AnimationKind::TwoRipples;
    app.animation_settings.density_percent = 100;
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer.content().iter().any(|cell| cell
            .symbol()
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))),
        "enabled ambient scene should occupy permitted blank cells"
    );
    let viewport = app.pane_viewports()[0];
    for x in viewport.content_area.x..viewport.content_area.x + 4 {
        let cell = &buffer[(x, viewport.content_area.y + 1)];
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.bg, ratatui::style::Color::Indexed(1));
    }
    let PaneRuntime::Terminal(source) = app.panes.get(&pane_id).unwrap() else {
        panic!("terminal fixture");
    };
    assert_eq!(source.with_screen(vt100::Screen::contents), original);
}

#[test]
#[ignore = "writes render receipts only when ILIUM_ZEN_EVIDENCE_DIR is supplied"]
fn capture_all_animation_settings_frames() {
    let destination = std::env::var_os("ILIUM_ZEN_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit evidence directory required");
    std::fs::create_dir_all(&destination).unwrap();
    let project = tempfile::tempdir().unwrap();
    for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
        for seconds in [0, 8] {
            let mut app = App::new("Animation review".into(), project.path().to_path_buf());
            app.set_screen_area(Rect::new(0, 0, 160, 50));
            app.started_at = Instant::now() - Duration::from_secs(seconds);
            app.animation_settings = AnimationSettings {
                kind,
                ..AnimationSettings::default()
            };
            app.mode = Mode::Settings(SettingsState {
                tab: SettingsTab::Animations,
                selected_row: index,
                ..Default::default()
            });
            let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let cells: Vec<_> = buffer
                .content()
                .iter()
                .map(|cell| {
                    serde_json::json!({
                        "symbol": cell.symbol(),
                        "fg": format!("{:?}", cell.fg),
                        "bg": format!("{:?}", cell.bg),
                        "modifier": format!("{:?}", cell.modifier),
                    })
                })
                .collect();
            let name = format!("scene-{:02}-{seconds:02}s.json", index + 1);
            let path = destination.join(name);
            let receipt = serde_json::json!({
                "type": "artifact", "scene": kind, "label": kind.label(),
                "seconds": seconds, "width": 160, "height": 50,
                "source": "actual ui::draw / ratatui TestBackend", "cells": cells,
            });
            std::fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            println!(
                "{}",
                serde_json::json!({"type":"artifact", "path":path,"scene":kind})
            );
        }
    }
}

#[test]
fn default_preview_dots_are_discrete_neutral_grey() {
    use ratatui::style::Color;
    let project = tempfile::tempdir().unwrap();
    let mut app = App::new("grey regression".into(), project.path().to_path_buf());
    app.set_screen_area(Rect::new(0, 0, 140, 40));
    app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Animations,
        ..Default::default()
    });
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let mut count = 0;
    for cell in terminal.backend().buffer().content() {
        if cell
            .symbol()
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        {
            count += 1;
            match cell.fg {
                Color::Rgb(r, g, b) => assert!(
                    r == g && g == b && (70..=190).contains(&r),
                    "discrete grey, got {r},{g},{b}"
                ),
                other => panic!("background preview must use calibrated grey, got {other:?}"),
            }
        }
    }
    assert!(count > 0);
}

#[test]
fn colour_controls_survive_scene_switch_and_project_reload() {
    let project = tempfile::tempdir().unwrap();
    let mut app = App::new("colour persistence".into(), project.path().to_path_buf());
    let settings: AnimationSettings = serde_json::from_value(serde_json::json!({
        "lightness_percent": 35, "hue_degrees": 275, "saturation_percent": 40
    }))
    .unwrap();
    crate::project_config::set_animation(project.path(), settings).unwrap();
    app.animation_settings = crate::project_config::load(project.path())
        .unwrap()
        .animation;
    app.settings_adjust_animation_row(5, 1);
    app.settings_adjust_animation_row(1, 1);
    let saved = crate::project_config::load(project.path())
        .unwrap()
        .animation;
    let value = serde_json::to_value(saved).unwrap();
    assert_eq!(value["lightness_percent"], 35);
    assert_eq!(value["hue_degrees"], 275);
    assert_eq!(value["saturation_percent"], 40);
    assert_eq!(saved.kind, AnimationKind::MoonlitWater);
    assert_eq!(
        crate::project_config::load(tempfile::tempdir().unwrap().path())
            .unwrap()
            .animation,
        AnimationSettings::default()
    );
}

#[test]
fn colour_slider_keyboard_changes_saved_lightness_without_changing_scene() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let project = tempfile::tempdir().unwrap();
    let mut app = App::new("slider keyboard".into(), project.path().to_path_buf());
    app.set_screen_area(Rect::new(0, 0, 80, 24));
    app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Animations,
        selected_row: 14,
        ..Default::default()
    });
    crate::keys::handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
    );
    let saved = crate::project_config::load(project.path())
        .unwrap()
        .animation;
    let value = serde_json::to_value(saved).unwrap();
    assert_eq!(value["lightness_percent"], 65);
    assert_eq!(saved.kind, AnimationKind::Shoreline);
    let Mode::Settings(state) = &app.mode else {
        panic!("slider keeps Settings open")
    };
    let area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
    assert!(crate::animation_settings_ui::row_y(area, 14, state.scroll).is_some());
}

#[test]
fn colour_slider_adjustment_failure_keeps_effective_ink_and_scene() {
    let project = tempfile::tempdir().unwrap();
    let mut app = App::new("failed colour save".into(), project.path().to_path_buf());
    std::fs::create_dir_all(project.path().join(".ilium/config.yaml")).unwrap();
    let prior = app.animation_settings;
    app.settings_adjust_animation_row(14, 1);
    assert_eq!(app.animation_settings, prior);
    assert!(app
        .status_message
        .as_deref()
        .is_some_and(|message| message.contains("Could not save")));
}
