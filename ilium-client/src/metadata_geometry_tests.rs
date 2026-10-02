//! Geometry regressions: metadata changes must never resize the child PTY.
use super::*;
use ilium_core::{PaneProgress, ProgressTaskReport, ProgressTaskStatus};
use ilium_ipc::ServerEvent;

fn terminal_app() -> (App, NodeId) {
    let mut app = App::new("geometry-regression".into(), std::env::temp_dir());
    let group = app.tree.add_group(ROOT_ID, "test").unwrap();
    let pane_id = app
        .tree
        .add_pane(group, "terminal", PaneContentKind::Terminal)
        .unwrap();
    app.panes.insert(
        pane_id,
        PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
    );
    app.right_panel_target = RightPanelTarget::Pane { pane_id };
    app.progress_display_now_unix_millis = 1000;
    app.set_screen_area(Rect::new(0, 0, 120, 40));
    app.take_outbound_requests();
    (app, pane_id)
}

#[test]
fn slots_are_reserved_before_agent_detection_or_first_prompt() {
    let (app, pane_id) = terminal_app();
    let viewport = app.pane_viewport(pane_id).unwrap();
    assert_eq!(
        viewport.last_prompt_area.unwrap().height,
        u16::from(app.ui_settings.last_prompt_max_lines)
    );
    assert_eq!(
        viewport.progress_area.unwrap().height,
        1 + u16::from(app.ui_settings.progress_max_lines)
    );
}

#[test]
fn prompt_and_progress_content_changes_do_not_resize_the_terminal() {
    let (mut app, pane_id) = terminal_app();
    let before = app.pane_viewport(pane_id).unwrap().content_area;
    for text in [
        "short".to_string(),
        "wrapped text ".repeat(300),
        String::new(),
    ] {
        crate::render_cache::apply(
            &mut app,
            ServerEvent::PaneLastPromptChanged {
                pane_id,
                last_prompt: Some(text),
            },
        );
        assert_eq!(app.pane_viewport(pane_id).unwrap().content_area, before);
        assert!(!app
            .take_outbound_requests()
            .iter()
            .any(|r| matches!(r, ClientRequest::ResizePane { .. })));
    }
    for message in ["short".to_string(), "long progress message ".repeat(80)] {
        let progress = PaneProgress::new(
            1,
            ProgressTaskReport {
                job_id: "geometry".into(),
                status: ProgressTaskStatus::Running,
                percent: 50.0,
                message,
                error: None,
            },
            1000,
        )
        .unwrap();
        crate::render_cache::apply(
            &mut app,
            ServerEvent::PaneProgressChanged {
                pane_id,
                progress: Some(progress),
            },
        );
        assert_eq!(app.pane_viewport(pane_id).unwrap().content_area, before);
        assert!(!app
            .take_outbound_requests()
            .iter()
            .any(|r| matches!(r, ClientRequest::ResizePane { .. })));
    }
    crate::render_cache::apply(
        &mut app,
        ServerEvent::PaneProgressChanged {
            pane_id,
            progress: None,
        },
    );
    assert_eq!(app.pane_viewport(pane_id).unwrap().content_area, before);
    assert!(!app
        .take_outbound_requests()
        .iter()
        .any(|r| matches!(r, ClientRequest::ResizePane { .. })));
}

#[test]
fn detection_and_exit_do_not_change_the_preallocated_terminal_rectangle() {
    let (mut app, pane_id) = terminal_app();
    let before = app.pane_viewport(pane_id).unwrap();
    for status in [
        ilium_core::PaneStatus::from_activity(
            ilium_core::AgentClass::Codex,
            ilium_core::AgentActivity::Working,
            None,
        ),
        ilium_core::PaneStatus::PlainShell,
    ] {
        crate::render_cache::apply(&mut app, ServerEvent::PaneStatusChanged { pane_id, status });
        assert_eq!(
            app.pane_viewport(pane_id).unwrap().content_area,
            before.content_area
        );
        assert!(!app
            .take_outbound_requests()
            .iter()
            .any(|r| matches!(r, ClientRequest::ResizePane { .. })));
    }
}

#[test]
fn hiding_a_completed_footer_preserves_its_slot_and_result() {
    let (mut app, pane_id) = terminal_app();
    let progress = PaneProgress::new(
        2,
        ProgressTaskReport {
            job_id: "expiry".into(),
            status: ProgressTaskStatus::Done,
            percent: 100.0,
            message: "finished marker".into(),
            error: None,
        },
        1000,
    )
    .unwrap();
    crate::render_cache::apply(
        &mut app,
        ServerEvent::PaneProgressChanged {
            pane_id,
            progress: Some(progress),
        },
    );
    app.take_outbound_requests();
    let before = app.pane_viewport(pane_id).unwrap();
    assert!(app.shows_progress_footer(pane_id));
    assert!(app.tick_completed_progress_display(61_000));
    assert!(!app.shows_progress_footer(pane_id));
    assert_eq!(app.pane_viewport(pane_id).unwrap(), before);
    assert!(app.tree.pane_progress(pane_id).is_some());
    assert!(app.take_outbound_requests().is_empty());
}

#[test]
fn an_explicit_slot_setting_change_still_resizes_the_child() {
    let (mut app, pane_id) = terminal_app();
    let before = app.pane_viewport(pane_id).unwrap().content_area.height;
    app.ui_settings.last_prompt_enabled = false;
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let after = app.pane_viewport(pane_id).unwrap();
    assert_eq!(
        after.content_area.height,
        before + u16::from(app.ui_settings.last_prompt_max_lines)
    );
    assert!(app.take_outbound_requests().iter().any(|r| matches!(r,
        ClientRequest::ResizePane { pane_id: id, rows, .. } if *id == pane_id && *rows == after.content_area.height
    )));
}

#[test]
fn a_server_confirmed_feature_setting_changes_geometry_once() {
    let (mut app, pane_id) = terminal_app();
    let before = app.pane_viewport(pane_id).unwrap().content_area.height;
    crate::render_cache::apply(
        &mut app,
        ServerEvent::ProgressMonitorEnabledChanged { enabled: false },
    );
    let after = app.pane_viewport(pane_id).unwrap();
    assert_eq!(
        after.content_area.height,
        before + 1 + u16::from(app.ui_settings.progress_max_lines)
    );
    assert!(app.take_outbound_requests().iter().any(
        |request| matches!(request, ClientRequest::ResizePane { pane_id: id, .. } if *id == pane_id)
    ));
    crate::render_cache::apply(
        &mut app,
        ServerEvent::ProgressMonitorEnabledChanged { enabled: false },
    );
    assert!(app.take_outbound_requests().is_empty());
}

#[test]
fn metadata_reservation_preserves_one_terminal_row_in_a_small_split() {
    let viewports = crate::split_layout::allocate_viewports(
        Rect::new(0, 0, 80, 12),
        ilium_core::SplitOrientation::Vertical,
        &[NodeId(1), NodeId(2)],
    );
    for viewport in viewports {
        assert!(viewport.content_area.height > 0);
        let with_slots = viewport
            .with_agent_toolbar_reserved()
            .with_last_prompt_reserved(4)
            .with_progress_reserved(5);
        assert!(with_slots.content_area.height >= 1);
        assert!(with_slots
            .outer_area
            .contains(ratatui::layout::Position::new(
                with_slots.content_area.x,
                with_slots.content_area.y
            )));
    }
}

#[test]
fn small_terminal_keeps_both_metadata_displays_and_a_terminal_row() {
    let (mut app, pane_id) = terminal_app();
    app.set_screen_area(Rect::new(0, 0, 120, 9));
    let viewport = app.pane_viewport(pane_id).unwrap();
    assert!(viewport.last_prompt_area.is_some());
    assert!(viewport.progress_area.is_some());
    assert_eq!(viewport.content_area.height, 1);
}

#[test]
fn genuine_outer_terminal_resize_updates_the_child_geometry() {
    let (mut app, pane_id) = terminal_app();
    let before = app.pane_viewport(pane_id).unwrap().content_area.height;
    app.set_screen_area(Rect::new(0, 0, 120, 50));
    let after = app.pane_viewport(pane_id).unwrap();
    assert_eq!(after.content_area.height, before + 10);
    assert!(app.take_outbound_requests().iter().any(|r| matches!(r,
        ClientRequest::ResizePane { pane_id: id, rows, .. } if *id == pane_id && *rows == after.content_area.height
    )));
}

#[test]
fn editors_and_boards_do_not_inherit_terminal_metadata_slots() {
    for kind in [PaneContentKind::Editor, PaneContentKind::Board] {
        let mut app = App::new("nonterminal-geometry".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "test").unwrap();
        let pane_id = app.tree.add_pane(group, "nonterminal", kind).unwrap();
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let viewport = app.pane_viewport(pane_id).unwrap();
        assert!(viewport.toolbar_area.is_none());
        assert!(viewport.last_prompt_area.is_none());
        assert!(viewport.progress_area.is_none());
    }
}
