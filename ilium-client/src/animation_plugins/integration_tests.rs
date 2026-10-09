//! Real App/input/persistence/widget contracts; these do not assert V8 output.
use super::*;
use crate::app::{App, Mode, SettingsState, SettingsTab};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;

fn app() -> (App, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("test project");
    let mut app = App::new("plugin-settings-contract".into(), project.path().into());
    app.set_screen_area(Rect::new(0, 0, 120, 40));
    app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Animations,
        ..Default::default()
    });
    (app, project)
}

#[test]
fn actual_plugin_issue_hover_dismisses_when_pointer_leaves_content() {
    let (mut app, _project) = app();
    app.plugin_catalogue = Some(
        crate::execution::test_client()
            .try_reserve_external(ilium_execution::JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .expect("catalogue admission")
            .retain(PluginCatalogue {
                entries: vec![],
                issues: vec![CatalogueIssue {
                    path: PathBuf::from("corrupt.iliumanim"),
                    message: "truncated ZIP".into(),
                }],
            })
            .expect("retained catalogue"),
    );
    let Mode::Settings(state) = &mut app.mode else {
        panic!("settings");
    };
    state.animation_source_tab = AnimationSourceTab::Plugin;
    for (column, row) in [(0, 0), (0, 10)] {
        let Mode::Settings(state) = &app.mode else {
            panic!("settings");
        };
        let layout =
            crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state);
        let panel = crate::animation_settings_ui::plugin_panel_area(layout.content_area);
        let issue_row = app
            .plugin_panel_model()
            .rows
            .iter()
            .position(|row| *row == PluginPanelRow::Issues)
            .expect("issue row");
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Moved,
                column: panel.x,
                row: panel.y + u16::try_from(issue_row).expect("visible issue row"),
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(
            app.plugin_issue_hover.is_some(),
            "pointer over issue row must establish hover"
        );
        app.tick_plugin_issue_hover(
            std::time::Instant::now() + crate::animation_hover::HOVER_DELAY,
        );
        assert!(app.plugin_issue_hover.is_some_and(|hover| hover.is_shown));
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(
            app.plugin_issue_hover.is_none(),
            "pointer at {column},{row} must dismiss issue popover"
        );
    }
}

fn retain_catalogue(app: &mut App, catalogue: PluginCatalogue) {
    app.plugin_catalogue = Some(
        crate::execution::test_client()
            .try_reserve_external(ilium_execution::JobCost {
                input_bytes: 4096,
                result_bytes: 1024 * 1024,
            })
            .expect("test admission")
            .retain(catalogue)
            .expect("retained catalogue"),
    );
}

fn install_metadata(app: &mut App) {
    let catalogue = PluginCatalogue {
        entries: [("beach", "Beach"), ("carpet", "Carpet")]
            .into_iter()
            .map(|(id, name)| PluginDescriptor {
                archive_path: PathBuf::from(format!("not-installed/{id}.iliumanim")),
                manifest: serde_json::from_value(json!({
                    "api_version":1,"id":id,"name":name,"version":"1.0.0",
                    "entry":"entry.mjs","modes":["live","pre_rendered"],"files":[],
                    "settings":{"type":"object","properties":{"speed":{"type":"integer","minimum":1,"maximum":10,"default":3}}}
                }))
                .expect("animation manifest"),
            })
            .collect(),
        issues: vec![],
    };
    retain_catalogue(app, catalogue);
}

fn install_official_catalogue(app: &mut App) {
    let bundled =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ilium-animation-js/assets/packages");
    let catalogue = PluginCatalogue::discover_with_bundled_and_stop(
        std::slice::from_ref(&bundled),
        Some(&bundled),
        || false,
    );
    assert!(catalogue.issues.is_empty(), "{:?}", catalogue.issues);
    for &(id, _, _) in release::PACKAGES {
        assert!(
            catalogue.find(id).is_some(),
            "official package {id} must be discoverable from its archive"
        );
    }
    retain_catalogue(app, catalogue);
}

#[test]
fn actual_keyboard_and_pointer_subtabs_browse_without_changing_source() {
    let (mut app, _project) = app();
    install_metadata(&mut app);
    let before = app.animation_settings.clone();
    crate::keys::handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
    );
    let Mode::Settings(state) = &app.mode else {
        panic!("settings");
    };
    assert_eq!(state.animation_source_tab, AnimationSourceTab::Plugin);
    assert_eq!(app.animation_settings, before);
    crate::keys::handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
    );
    assert_eq!(app.animation_settings, before);
    let Mode::Settings(state) = &app.mode else {
        panic!("settings");
    };
    let area = crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
        .content_area;
    let tabs = source_tabs(crate::animation_settings_ui::source_tab_area(area));
    crate::mouse::handle_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tabs.native.x,
            row: tabs.native.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    let Mode::Settings(state) = &app.mode else {
        panic!("settings");
    };
    assert_eq!(state.animation_source_tab, AnimationSourceTab::Native);
    assert_eq!(app.animation_settings, before);
}

#[test]
fn actual_plugin_panel_renders_catalogue_and_shared_controls_at_terminal_sizes() {
    for (width, height) in [(40, 16), (80, 24), (120, 40)] {
        let (mut app, _project) = app();
        install_official_catalogue(&mut app);
        app.set_screen_area(Rect::new(0, 0, width, height));
        let Mode::Settings(state) = &mut app.mode else {
            panic!("settings");
        };
        state.animation_source_tab = AnimationSourceTab::Plugin;
        let model = app.plugin_panel_model();
        assert!(model
            .rows
            .iter()
            .any(|row| matches!(row, PluginPanelRow::Common(_))));
        for id in ["beach", "carpet"] {
            assert_eq!(
                model
                    .rows
                    .iter()
                    .filter(|row| *row == &PluginPanelRow::Package(id.into()))
                    .count(),
                1,
                "each requested animation must appear once"
            );
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| {
                let Mode::Settings(state) = &app.mode else {
                    panic!("settings");
                };
                crate::animation_settings_ui::render(frame, frame.area(), &app, state);
            })
            .expect("real plugin panel");
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Native"));
        assert!(text.contains("Plugin"));
        assert!(text.contains("Beach"));
        assert!(text.contains("Carpet"));
        assert_eq!(app.animation_settings.source, AnimationSourceTab::Native);
    }
}

#[test]
fn actual_package_row_click_selects_and_persists_official_beach_and_carpet() {
    for package_id in ["beach", "carpet"] {
        let (mut app, project) = app();
        install_official_catalogue(&mut app);
        app.set_screen_area(Rect::new(0, 0, 80, 24));
        let content_area = {
            let Mode::Settings(state) = &app.mode else {
                panic!("settings");
            };
            crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
                .content_area
        };
        let tabs = source_tabs(crate::animation_settings_ui::source_tab_area(content_area));
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: tabs.plugin.x,
                row: tabs.plugin.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("settings");
        };
        assert_eq!(state.animation_source_tab, AnimationSourceTab::Plugin);
        let row = app
            .plugin_panel_model()
            .rows
            .iter()
            .position(|row| row == &PluginPanelRow::Package(package_id.into()))
            .expect("official package row");
        let package_row = crate::animation_plugins::plugin_row_rect(
            crate::animation_settings_ui::plugin_panel_area(content_area),
            0,
            row,
        )
        .expect("visible package row");
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: package_row.x,
                row: package_row.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert_eq!(app.animation_settings.source, AnimationSourceTab::Plugin);
        assert_eq!(
            app.animation_settings
                .plugin
                .selected
                .as_ref()
                .map(|selection| selection.package_id.as_str()),
            Some(package_id)
        );
        let tabs = source_tabs(crate::animation_settings_ui::source_tab_area(content_area));
        crate::mouse::handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: tabs.native.x,
                row: tabs.native.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("settings");
        };
        assert_eq!(state.animation_source_tab, AnimationSourceTab::Native);
        assert_eq!(app.animation_settings.source, AnimationSourceTab::Plugin);
        assert_eq!(
            app.animation_settings
                .plugin
                .selected
                .as_ref()
                .map(|selection| selection.package_id.as_str()),
            Some(package_id)
        );

        app.settle_filesystem_for_test();
        let saved = crate::project_config::load(project.path())
            .expect("authoritative saved settings")
            .animation;
        assert_eq!(saved.source, AnimationSourceTab::Plugin);
        assert_eq!(
            saved
                .plugin
                .selected
                .as_ref()
                .map(|selection| selection.package_id.as_str()),
            Some(package_id)
        );
    }
}

#[test]
fn selecting_native_even_with_same_kind_persists_explicit_native_source() {
    let (mut app, project) = app();
    app.animation_settings.source = AnimationSourceTab::Plugin;
    let kind = app.animation_settings.kind;
    app.settings_select_animation_scene(kind);
    app.settle_filesystem_for_test();
    let saved = crate::project_config::load(project.path())
        .expect("authoritative saved settings")
        .animation;
    assert_eq!(saved.source, AnimationSourceTab::Native);
    assert_eq!(saved.kind, kind);
}
