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
    for last_prompt in [
        Some("short".to_string()),
        Some("wrapped text ".repeat(300)),
        Some(String::new()),
        None,
    ] {
        crate::render_cache::apply(
            &mut app,
            ServerEvent::PaneLastPromptChanged {
                pane_id,
                last_prompt,
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
                details: String::new(),
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
            details: String::new(),
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
fn rejected_resize_preserves_geometry_and_can_be_retried_after_admission_returns() {
    let (mut app, pane_id) = terminal_app();
    // A shared general test client lets unrelated parallel tests consume the
    // credits needed by this retry assertion. Give this fixture its own tenant.
    app.outbound_admission = Some(crate::execution::test_document_client());
    let before = app.requested_pane_sizes[&pane_id];
    let admission = app.outbound_admission.take();
    assert!(
        admission.is_some(),
        "fixture must start with outbound credits"
    );
    app.ui_settings.last_prompt_enabled = false;
    let viewport = app.pane_viewport(pane_id).unwrap();
    let desired = (
        viewport.content_area.height.max(1),
        viewport.content_area.width.max(1),
    );
    assert_ne!(desired, before);

    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    assert!(app.take_outbound_requests().is_empty());
    assert_eq!(
        app.requested_pane_sizes[&pane_id], before,
        "a rejected resize must not be credited as queued"
    );
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
        panic!("terminal fixture disappeared");
    };
    assert_eq!(
        view.desired_size, before,
        "local geometry must not advance without outbound resize admission"
    );

    app.outbound_admission = admission;
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let requests = app.take_outbound_requests();
    assert_eq!(requests.len(), 1, "retry must publish exactly one resize");
    assert!(matches!(
        requests[0],
        ClientRequest::ResizePane { pane_id: id, rows, cols, .. }
            if id == pane_id && (rows, cols) == desired
    ));
    assert_eq!(app.requested_pane_sizes[&pane_id], desired);
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    assert!(app.take_outbound_requests().is_empty());
}

#[test]
fn exhausted_outbound_credits_preserve_resize_geometry_until_release() {
    let (mut app, pane_id) = terminal_app();
    // Exhaust only this test's tenant, leaving parallel tests independent.
    app.outbound_admission = Some(crate::execution::test_document_client());
    let before = app.requested_pane_sizes[&pane_id];
    let client = app.outbound_admission.as_ref().unwrap().clone();
    let mut held = Vec::new();
    let mut refused = false;
    for _ in 0..256 {
        match client.try_reserve_external(ilium_execution::JobCost {
            input_bytes: 4096,
            result_bytes: 4096,
        }) {
            Ok(reservation) => held.push(reservation),
            Err(_) => {
                refused = true;
                break;
            }
        }
    }
    assert!(refused, "fixture must exhaust its finite outbound credits");
    app.ui_settings.last_prompt_enabled = false;
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    assert!(app.take_outbound_requests().is_empty());
    assert_eq!(app.requested_pane_sizes[&pane_id], before);
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
        panic!("terminal fixture disappeared");
    };
    assert_eq!(view.desired_size, before);

    drop(held);
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let requests = app.take_outbound_requests();
    assert_eq!(requests.len(), 1);
    assert!(matches!(requests[0], ClientRequest::ResizePane { pane_id: id, .. } if id == pane_id));
}

#[test]
fn local_geometry_rejection_does_not_reserve_outbound_credit() {
    let (mut app, pane_id) = terminal_app();
    let before = app.requested_pane_sizes[&pane_id];
    // Keep this accounting assertion isolated from other App tests using the
    // shared default test tenant concurrently.
    app.outbound_admission = Some(crate::execution::test_document_client());
    let client = app.outbound_admission.as_ref().unwrap().clone();
    let usage = client.usage();
    app.set_screen_area(Rect::new(0, 0, 4096, 4096));
    assert!(app.take_outbound_requests().is_empty());
    assert_eq!(app.requested_pane_sizes[&pane_id], before);
    let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
        panic!("terminal fixture disappeared");
    };
    assert_eq!(view.desired_size, before);
    assert_eq!(client.usage().jobs, usage.jobs);
    assert_eq!(client.usage().input_bytes, usage.input_bytes);
    assert_eq!(client.usage().result_bytes, usage.result_bytes);
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

// Synthetic wire fixture: resize refusals must identify the requested geometry
// instead of invalidating unrelated panes through a generic status message.
fn refused_resize_event(pane_id: NodeId, size: (u16, u16)) -> ServerEvent {
    serde_json::from_value(serde_json::json!({
        "PaneResizeRejected": {
            "pane_id": pane_id,
            "rows": size.0,
            "cols": size.1,
            "message": "forced resize refusal"
        }
    }))
    .expect("the protocol must preserve pane-specific resize refusals")
}

#[test]
fn a_server_refused_resize_can_be_explicitly_retried_without_replaying_input() {
    let (mut app, pane_id) = terminal_app();
    app.ui_settings.last_prompt_enabled = false;
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let requested = app.requested_pane_sizes[&pane_id];
    let first = app.take_outbound_requests();
    assert!(first.iter().any(|request| matches!(request,
        ClientRequest::ResizePane { pane_id: id, rows, cols, .. }
            if *id == pane_id && (*rows, *cols) == requested
    )));

    crate::render_cache::apply(&mut app, refused_resize_event(pane_id, requested));
    assert!(!app.requested_pane_sizes.contains_key(&pane_id));
    assert!(app
        .status_message
        .as_deref()
        .unwrap()
        .contains("forced resize refusal"));
    assert!(
        app.take_outbound_requests().is_empty(),
        "refusal must not replay input or resize automatically"
    );

    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let retried = app.take_outbound_requests();
    assert_eq!(retried.len(), 1);
    assert!(matches!(retried[0],
        ClientRequest::ResizePane { pane_id: id, rows, cols, .. }
            if id == pane_id && (rows, cols) == requested
    ));
}

#[test]
fn an_old_resize_refusal_does_not_invalidate_a_newer_requested_geometry() {
    let (mut app, pane_id) = terminal_app();
    let old = app.requested_pane_sizes[&pane_id];
    app.ui_settings.last_prompt_enabled = false;
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    let newer = app.requested_pane_sizes[&pane_id];
    assert_ne!(old, newer);
    app.take_outbound_requests();

    crate::render_cache::apply(&mut app, refused_resize_event(pane_id, old));
    assert_eq!(app.requested_pane_sizes[&pane_id], newer);
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    assert!(app.take_outbound_requests().is_empty());
}

#[test]
fn an_unrelated_server_error_does_not_cause_terminal_resize_churn() {
    let (mut app, pane_id) = terminal_app();
    let requested = app.requested_pane_sizes[&pane_id];
    crate::render_cache::apply(
        &mut app,
        ServerEvent::Error {
            message: "synthetic unrelated request failure".into(),
        },
    );
    assert_eq!(app.requested_pane_sizes[&pane_id], requested);
    app.resize_displayed_panes(PaneResizeCause::UserInterfaceSettings);
    assert!(app.take_outbound_requests().is_empty());
}
