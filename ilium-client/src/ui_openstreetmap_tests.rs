//! Final composed OpenStreetMap attribution and protected terminal geometry.
use super::*;
use crate::app::{SettingsState, SettingsTab};
use crate::background_animation::AnimationKind;
use crate::terminal_view::TerminalView;
use ilium_core::PaneContentKind;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn test_app(width: u16, height: u16) -> (App, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("isolated project");
    let mut app = App::new("osm-credit".to_owned(), project.path().to_path_buf());
    app.animation_settings.kind = AnimationKind::OpenStreetMap;
    app.animation_settings.enabled = true;
    app.set_screen_area(Rect::new(0, 0, width, height));
    (app, project)
}

fn draw_buffer(terminal: &mut Terminal<TestBackend>, app: &mut App) {
    terminal
        .draw(|frame| draw_at(frame, app, Duration::ZERO))
        .expect("final UI render");
}

fn draw_loaded_map(terminal: &mut Terminal<TestBackend>, app: &mut App) {
    let started = Instant::now();
    loop {
        terminal
            .draw(|frame| draw_at(frame, app, started.elapsed()))
            .expect("loaded map render");
        if app
            .animation_frame
            .status()
            .is_some_and(|status| status.contains("Paris ·"))
            && app
                .animation_frame
                .packed_cells()
                .iter()
                .any(|bits| *bits != 0)
        {
            assert!(whole_text(terminal.backend().buffer())
                .chars()
                .any(|glyph| ('\u{2801}'..='\u{28ff}').contains(&glyph)));
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "offline map did not load"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn row_text(buffer: &Buffer, area: Rect, row: u16) -> String {
    (area.left()..area.right())
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

fn whole_text(buffer: &Buffer) -> String {
    (buffer.area.top()..buffer.area.bottom())
        .map(|row| row_text(buffer, buffer.area, row))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_credit(buffer: &Buffer, area: Rect) {
    assert!(!area.is_empty(), "credit needs reserved geometry");
    let credit = (area.top()..area.bottom())
        .map(|row| row_text(buffer, area, row))
        .collect::<String>();
    assert_eq!(credit.trim_end(), crate::layout::OSM_ATTRIBUTION);
    for row in area.top()..area.bottom() {
        for column in area.left()..area.right() {
            let cell = &buffer[(column, row)];
            assert_ne!(cell.fg, cell.bg, "credit contrast at {column},{row}");
            assert_ne!(cell.bg, Color::Reset, "credit strip at {column},{row}");
        }
    }
}

#[test]
fn ordinary_osm_credit_survives_final_composition_with_pty_footer_and_voice() {
    for (width, height) in [(80, 24), (120, 40)] {
        let (mut app, _project) = test_app(width, height);
        let group = app.tree.add_group(ROOT_ID, "work").expect("group");
        let pane_id = app
            .tree
            .add_pane(group, "native terminal", PaneContentKind::Terminal)
            .expect("pane");
        let (rows, columns) = app.layout.pane_content_size();
        let mut view = TerminalView::new(rows, columns);
        view.apply_replay("PTY 中".as_bytes(), 1, true);
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        if width == 120 {
            let now = chrono::Utc::now();
            app.reset_monitor_state.codex.scheduled = Some(crate::reset_planning::ScheduledReset {
                announced_at: now,
                scheduled_for: Some(now + chrono::Duration::hours(2)),
                source_url: "https://example.invalid/reset".to_owned(),
                is_banked: false,
            });
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        draw_loaded_map(&mut terminal, &mut app);
        let buffer = terminal.backend().buffer();
        let credit_area = app.layout.osm_attribution_area;
        assert_credit(buffer, credit_area);
        assert_eq!(app.layout.pane_area.bottom(), credit_area.top());
        assert_eq!(app.layout.status_area.y, height - 1);
        assert_eq!(app.layout.voice_control_area.y, height - 1);
        let content = app.pane_viewport(pane_id).expect("viewport").content_area;
        assert_eq!(buffer[(content.x, content.y)].symbol(), "P");
        assert_eq!(buffer[(content.x + 4, content.y)].symbol(), "中");
        assert_eq!(buffer[(content.x + 5, content.y)].symbol(), " ");
        // The terminal widget's default block cursor must survive the map.
        assert_eq!(buffer[(content.x + 6, content.y)].symbol(), "█");
        let footer = row_text(buffer, app.layout.voice_control_area, height - 1);
        assert!(footer.contains("VOICE"), "{width}x{height}: {footer}");
        if width == 120 {
            assert!(row_text(buffer, app.layout.status_area, height - 1).contains("Codex reset in"));
        }
        app.animation_settings
            .ambient
            .openstreetmap
            .brightness_percent = 0;
        draw_buffer(&mut terminal, &mut app);
        assert!(app
            .animation_frame
            .packed_cells()
            .iter()
            .all(|bits| *bits == 0));
        assert_credit(terminal.backend().buffer(), credit_area);
    }
}

#[test]
fn settings_and_fullscreen_osm_credit_have_separate_hint_and_voice_rows() {
    for (width, height, fullscreen) in [
        (80, 24, false),
        (120, 40, false),
        (20, 10, true),
        (80, 24, true),
    ] {
        let (mut app, _project) = test_app(width, height);
        app.animation_settings.enabled = false; // Preview remains explicit.
        app.set_screen_area(Rect::new(0, 0, width, height));
        let state = SettingsState {
            tab: SettingsTab::Animations,
            animation_fullscreen: fullscreen,
            ..Default::default()
        };
        app.mode = Mode::Settings(state);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        draw_buffer(&mut terminal, &mut app);
        let buffer = terminal.backend().buffer();
        let credit_area = crate::layout::osm_attribution_area(buffer.area);
        assert_credit(buffer, credit_area);
        assert!(row_text(buffer, app.layout.voice_control_area, height - 1).contains("VOICE"));
        if fullscreen {
            let hint_row = credit_area.top() - 1;
            assert!(row_text(buffer, buffer.area, hint_row).contains("Full"));
        } else {
            let Mode::Settings(state) = &app.mode else {
                panic!("settings mode survives drawing");
            };
            let settings = crate::settings_ui::compute_layout_for_mode(buffer.area, &app, state);
            assert_eq!(settings.content_area.bottom(), credit_area.top());
            let model = app.animation_row_model();
            assert!(crate::animation_settings_ui::hit(
                settings.content_area,
                &model,
                crate::animation_settings_ui::Scrolls::default(),
                Position::new(credit_area.x, credit_area.y),
            )
            .is_none());
        }
    }
}

#[test]
fn disabled_or_switched_scene_clears_credit_and_restores_workspace_height() {
    let (mut app, project) = test_app(80, 24);
    let group = app.tree.add_group(ROOT_ID, "resize").expect("group");
    let pane_id = app
        .tree
        .add_pane(group, "terminal", PaneContentKind::Terminal)
        .expect("pane");
    let (rows, columns) = app.layout.pane_content_size();
    app.panes.insert(
        pane_id,
        PaneRuntime::Terminal(Box::new(TerminalView::new(rows, columns))),
    );
    app.right_panel_target = RightPanelTarget::Pane { pane_id };
    let ordinary_height = app.layout.pane_area.height + 1;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
    draw_buffer(&mut terminal, &mut app);
    assert_credit(terminal.backend().buffer(), app.layout.osm_attribution_area);

    let background_row = app
        .animation_row_model()
        .rows()
        .iter()
        .position(|row| {
            matches!(
                row,
                crate::animation_rows::AnimationRow::Common("background")
            )
        })
        .expect("background control");
    app.take_outbound_requests();
    app.settings_adjust_animation_row(background_row, 1);
    assert!(!app.animation_settings.enabled);
    assert!(
        !crate::project_config::load(project.path())
            .unwrap()
            .animation
            .enabled
    );
    let viewport = app.pane_viewport(pane_id).expect("resized viewport");
    assert!(app.take_outbound_requests().iter().any(|request| matches!(
        request,
        ilium_ipc::ClientRequest::ResizePane { pane_id: resized, rows, cols, cause }
            if *resized == pane_id
                && *rows == viewport.content_area.height
                && *cols == viewport.content_area.width
                && *cause == ilium_ipc::PaneResizeCause::UserInterfaceSettings
    )));
    draw_buffer(&mut terminal, &mut app);
    assert!(app.layout.osm_attribution_area.is_empty());
    assert_eq!(app.layout.pane_area.height, ordinary_height);
    assert!(!whole_text(terminal.backend().buffer()).contains(crate::layout::OSM_ATTRIBUTION));

    app.settings_adjust_animation_row(background_row, 1);
    assert!(app.animation_settings.enabled);
    app.settings_select_animation_scene(AnimationKind::QuietPond);
    assert_eq!(
        crate::project_config::load(project.path())
            .unwrap()
            .animation
            .kind,
        AnimationKind::QuietPond
    );
    draw_buffer(&mut terminal, &mut app);
    assert!(app.layout.osm_attribution_area.is_empty());
    assert!(!whole_text(terminal.backend().buffer()).contains(crate::layout::OSM_ATTRIBUTION));
}

#[test]
fn too_small_to_credit_suppresses_the_map() {
    let (mut app, _project) = test_app(10, 4);
    let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("backend");
    draw_buffer(&mut terminal, &mut app);
    assert!(crate::layout::osm_attribution_area(Rect::new(0, 0, 10, 4)).is_empty());
    assert_eq!(app.animation_frame.width(), 0);
}

#[test]
fn osm_settings_help_links_the_credited_source_and_license() {
    let topic = crate::settings_help::catalog::by_id("AN-55").expect("OSM help topic");
    assert!(topic.caveat.contains("OpenStreetMap contributors / ODbL"));
    assert!(topic
        .caveat
        .contains("https://www.openstreetmap.org/copyright"));
}
