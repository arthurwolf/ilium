use crate::app::{App, Mode, SettingsState, SettingsTab};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::cell::RefCell;
thread_local! {
    #[allow(non_upper_case_globals)]
    static opened_topic_slot: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
}
pub(crate) fn opened_topic(topic_id: &str) {
    opened_topic_slot.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(topic) = slot.as_mut() else {
            return;
        };
        assert!(topic.is_none(), "one input opened more than one help topic");
        *topic = Some(topic_id.to_owned());
    });
}
fn observe_open(action: impl FnOnce()) -> Option<String> {
    opened_topic_slot.with(|slot| assert!(slot.replace(Some(None)).is_none()));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    let topic = opened_topic_slot.with(|slot| slot.take().expect("active help probe"));
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
    topic
}
fn help_app(screen: Rect, selected_row: usize, scroll: u16) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new("terminal-pool-help".into(), directory.path().into());
    app.config_dir = Some(directory.path().into());
    app.set_screen_area(screen);
    app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Terminal,
        selected_row,
        scroll,
        scene_scroll: 6,
        global_scroll: 7,
        icons_preview_real: true,
        trigger_action_cursor: 2,
        ..SettingsState::default()
    });
    (directory, app)
}
fn pool_help_position(app: &App) -> Position {
    let Mode::Settings(state) = &app.mode else {
        panic!("terminal settings expected");
    };
    let layout = crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, app, state);
    let anchors = crate::settings_ui::settings_help_anchors(&layout, app, state);
    let matches: Vec<_> = anchors
        .iter()
        .filter(|anchor| anchor.topic_id == "TERM-03")
        .collect();
    assert_eq!(matches.len(), 1, "one visible parser-pool help anchor");
    let anchor = matches[0];
    assert_eq!(anchor.selected, state.selected_row == 4);
    if state.selected_row == 4 {
        assert_eq!(anchors.iter().filter(|anchor| anchor.selected).count(), 1);
    }
    assert_eq!(anchor.hit_area.y, layout.content_area.y + 13 - state.scroll);
    assert_eq!(anchor.hit_area.x, layout.help_rail_area.x);
    assert_eq!(anchor.hit_area.width, layout.help_rail_area.width);
    assert!(anchor.hit_area.y < layout.content_area.bottom());
    let position = Position::new(anchor.hit_area.x, anchor.hit_area.y);
    assert_eq!(
        crate::settings_ui::settings_help_at(&layout, app, state, position).as_deref(),
        Some("TERM-03")
    );
    let outside = Position::new(anchor.hit_area.right(), anchor.hit_area.y);
    assert!(crate::settings_ui::settings_help_at(&layout, app, state, outside).is_none());
    position
}
fn assert_restored(app: &App, screen: Rect, selected_row: usize, scroll: u16) {
    let Mode::Settings(state) = &app.mode else {
        panic!("help must restore settings");
    };
    assert_eq!(state.tab, SettingsTab::Terminal);
    assert_eq!(state.selected_row, selected_row);
    assert_eq!(state.scroll, scroll);
    assert_eq!(state.scene_scroll, 6);
    assert_eq!(state.global_scroll, 7);
    assert!(state.icons_preview_real);
    assert_eq!(state.trigger_action_cursor, 2);
    assert_eq!(app.layout.screen_area, screen);
}
#[test]
fn terminal_pool_help_anchor_respects_scroll_and_viewport_clipping() {
    for screen in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 120, 40)] {
        for scroll in [10, 13] {
            let (_directory, app) = help_app(screen, 4, scroll);
            pool_help_position(&app);
        }
        let (_directory, app) = help_app(screen, 4, 14);
        let Mode::Settings(state) = &app.mode else {
            panic!("settings expected");
        };
        let layout = crate::settings_ui::compute_layout_for_mode(screen, &app, state);
        assert!(crate::settings_ui::settings_help_anchors(&layout, &app, state).is_empty());
        let top = Position::new(layout.help_rail_area.x, layout.content_area.y);
        assert!(crate::settings_ui::settings_help_at(&layout, &app, state, top).is_none());
    }
    let screen = Rect::new(0, 0, 80, 12);
    let (_directory, app) = help_app(screen, 4, 0);
    let Mode::Settings(state) = &app.mode else {
        panic!("settings expected");
    };
    let layout = crate::settings_ui::compute_layout_for_mode(screen, &app, state);
    assert!(layout.content_area.height <= 13);
    assert!(
        crate::settings_ui::settings_help_anchors(&layout, &app, state)
            .iter()
            .all(|anchor| anchor.topic_id != "TERM-03")
    );
}
#[test]
fn terminal_pool_question_mark_opens_the_selected_topic_and_restores_navigation() {
    for (screen, scroll, modifiers) in [
        (Rect::new(0, 0, 80, 24), 10, KeyModifiers::NONE),
        (Rect::new(0, 0, 120, 40), 0, KeyModifiers::SHIFT),
    ] {
        let (_directory, mut app) = help_app(screen, 4, scroll);
        let position = pool_help_position(&app);
        let settings = app.terminal_settings;
        let topic = observe_open(|| {
            crate::keys::handle_event(
                &mut app,
                Event::Key(KeyEvent::new(KeyCode::Char('?'), modifiers)),
            );
        });
        assert_eq!(topic.as_deref(), Some("TERM-03"));
        assert!(matches!(&app.mode, Mode::SettingsHelp(_)));
        app.pop_modal();
        assert_restored(&app, screen, 4, scroll);
        assert_eq!(pool_help_position(&app), position);
        assert_eq!(app.terminal_settings, settings);
    }
}
#[test]
fn terminal_pool_help_click_opens_the_hit_topic_and_restores_navigation() {
    for (screen, scroll) in [(Rect::new(0, 0, 80, 24), 10), (Rect::new(0, 0, 120, 40), 0)] {
        for selected_row in [0, 4] {
            let (_directory, mut app) = help_app(screen, selected_row, scroll);
            let position = pool_help_position(&app);
            let settings = app.terminal_settings;
            // Bind pointer dispatch to the fixture's current settings presentation.
            let emitted = app.capture_emitted_geometry(1);
            app.commit_emitted_geometry(emitted);
            assert!(app.pointer_geometry_is_current());
            let topic = observe_open(|| {
                crate::mouse::handle_mouse_event(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: position.x,
                        row: position.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
            });
            assert_eq!(topic.as_deref(), Some("TERM-03"));
            assert!(matches!(&app.mode, Mode::SettingsHelp(_)));
            app.pop_modal();
            assert_restored(&app, screen, selected_row, scroll);
            assert_eq!(pool_help_position(&app), position);
            assert_eq!(app.terminal_settings, settings);
        }
    }
}
