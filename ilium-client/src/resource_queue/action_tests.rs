//! Synthetic tree interaction acceptance through the client event/action routes.
use super::*;
use crate::app::{ContextMenuAction, Mode};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ilium_core::PaneContentKind;
use ilium_ipc::ClientRequest;
use ratatui::{backend::TestBackend, layout::Position, Terminal};

struct Fixture {
    app: App,
    project: NodeId,
    local: NodeId,
    ordinary: NodeId,
    resource: NodeId,
    local_row: NodeId,
    reference: NodeId,
}

fn fixture() -> Fixture {
    let (mut tree, mut catalog, project, local) = tree::tests::fixture();
    let ordinary = tree
        .add_pane(project, "ordinary shell", PaneContentKind::Terminal)
        .unwrap();
    let record = catalog.resources.get_mut(&ResourceId::from("gpu")).unwrap();
    let proof = record.reservations[0].agent.clone();
    // The foreign invocation deliberately reuses the local pane number. Only
    // the complete verified identity may make a participant actionable locally.
    let mut foreign = record.reservations[0].clone();
    foreign.id = ReservationId::from("foreign");
    foreign.request_id = RequestId::from("foreign-request");
    foreign.agent.session_id = "another-session".into();
    foreign.agent.label = "Remote participant".into();
    foreign.ticket = 3;
    record.reservations.push(foreign);
    let mut app = App::new("test".into(), "/synthetic-resource-project".into());
    app.tree = tree;
    app.ui_settings.resource_queue_grouping = true;
    app.receive_resource_catalog(
        catalog,
        ResourceClock {
            unix_millis: 1000,
            monotonic_millis: 1000,
            boot_id: "boot".into(),
        },
        vec![proof],
        vec![(local, ReservationId::from("reservation"))],
        0,
    );
    let row = |reservation: Option<&str>| {
        *app.resources
            .rows
            .targets
            .iter()
            .find(|(_, target)| match (reservation, target) {
                (None, ResourceRowTarget::Resource { .. }) => true,
                (Some(expected), ResourceRowTarget::Participant { reservation_id, .. }) => {
                    reservation_id.0 == expected
                }
                _ => false,
            })
            .unwrap()
            .0
    };
    let resource = row(None);
    let local_row = row(Some("reservation"));
    let reference = row(Some("foreign"));
    // Focus records activity acknowledgement on real panes. Establish that
    // legitimate metadata before asserting projection-only tree invariance.
    app.focus_pane(local);
    app.focus_pane(ordinary);
    app.select_tree_path(vec![project, resource, local_row]);
    app.focus = FocusTarget::Tree;
    render(&mut app);
    app.take_outbound_requests();
    Fixture {
        app,
        project,
        local,
        ordinary,
        resource,
        local_row,
        reference,
    }
}

fn render(app: &mut App) {
    app.set_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
    // The client rejects old frame acknowledgements; every rendered test
    // frame must advance its revision just like the real output owner.
    let revision = app.emitted_layout_revision().unwrap_or(0) + 1;
    app.commit_emitted_geometry(app.capture_emitted_geometry(revision));
    assert_eq!(app.emitted_layout_revision(), Some(revision));
}

fn key(app: &mut App, code: KeyCode) {
    crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn position(app: &mut App, target: NodeId) -> Position {
    render(app);
    let area = crate::tree_ui::list_area(app.layout.tree_area);
    (area.y..area.bottom())
        .map(|y| Position::new(area.x + 4, y))
        .find(|point| app.tree_node_at(*point).is_some_and(|hit| hit.id == target))
        .expect("requested row is actually emitted in the tree")
}

fn mouse(app: &mut App, point: Position, kind: MouseEventKind) {
    crate::mouse::handle_mouse_event(
        app,
        MouseEvent {
            kind,
            column: point.x,
            row: point.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}

#[test]
fn arrows_and_enter_cross_resource_local_reference_and_real_rows_without_mutating_tree() {
    let mut fixture = fixture();
    let before = serde_json::to_value(&fixture.app.tree).unwrap();
    fixture.app.select_tree_path(vec![fixture.project]);
    render(&mut fixture.app);
    for expected in [
        fixture.resource,
        fixture.local_row,
        fixture.reference,
        fixture.ordinary,
    ] {
        key(&mut fixture.app, KeyCode::Down);
        assert_eq!(fixture.app.tree_state.selected().last(), Some(&expected));
    }
    for expected in [
        fixture.reference,
        fixture.local_row,
        fixture.resource,
        fixture.project,
    ] {
        key(&mut fixture.app, KeyCode::Up);
        assert_eq!(fixture.app.tree_state.selected().last(), Some(&expected));
    }
    for (row, expected_pane) in [
        (fixture.resource, None),
        (fixture.reference, None),
        (fixture.local_row, Some(fixture.local)),
        (fixture.ordinary, Some(fixture.ordinary)),
    ] {
        let path = if row == fixture.ordinary || row == fixture.resource {
            vec![fixture.project, row]
        } else {
            vec![fixture.project, fixture.resource, row]
        };
        fixture.app.select_tree_path(path);
        fixture.app.focus = FocusTarget::Tree;
        key(&mut fixture.app, KeyCode::Enter);
        assert_eq!(
            fixture.app.right_panel_target.active_pane_id(),
            expected_pane
        );
        if expected_pane.is_none() {
            assert!(
                matches!(&fixture.app.right_panel_target, RightPanelTarget::Resource { project_id, resource_id } if *project_id == fixture.project && resource_id.0 == "gpu")
            );
        }
    }
    assert_eq!(serde_json::to_value(&fixture.app.tree).unwrap(), before);
    assert!(!fixture
        .app
        .take_outbound_requests()
        .iter()
        .any(|request| matches!(request, ClientRequest::NewPane { .. })));
}

#[test]
fn home_and_end_reach_visible_tree_boundaries_across_virtual_rows() {
    let mut fixture = fixture();
    key(&mut fixture.app, KeyCode::End);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.ordinary)
    );
    key(&mut fixture.app, KeyCode::Home);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.project)
    );
    key(&mut fixture.app, KeyCode::Left);
    render(&mut fixture.app);
    key(&mut fixture.app, KeyCode::End);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.project)
    );
    key(&mut fixture.app, KeyCode::Right);
    render(&mut fixture.app);
    key(&mut fixture.app, KeyCode::End);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.ordinary)
    );
    assert!(fixture.app.take_outbound_requests().is_empty());
}

#[test]
fn collapsing_resource_hides_participants_and_reopening_restores_navigation_without_ipc() {
    let mut fixture = fixture();
    fixture
        .app
        .select_tree_path(vec![fixture.project, fixture.resource]);
    key(&mut fixture.app, KeyCode::Left);
    render(&mut fixture.app);
    key(&mut fixture.app, KeyCode::Down);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.ordinary)
    );
    key(&mut fixture.app, KeyCode::Up);
    key(&mut fixture.app, KeyCode::Right);
    render(&mut fixture.app);
    key(&mut fixture.app, KeyCode::Down);
    assert_eq!(
        fixture.app.tree_state.selected().last(),
        Some(&fixture.local_row)
    );
    assert!(fixture.app.take_outbound_requests().is_empty());
}

#[test]
fn context_actions_resolve_resource_and_reference_to_project_and_local_row_to_real_pane() {
    for kind in ["resource", "reference", "local", "ordinary"] {
        for action in [
            ContextMenuAction::Rename,
            ContextMenuAction::Close,
            ContextMenuAction::Search,
        ] {
            let mut fixture = fixture();
            let (row, expected) = match kind {
                "resource" => (fixture.resource, fixture.project),
                "reference" => (fixture.reference, fixture.project),
                "local" => (fixture.local_row, fixture.local),
                _ => (fixture.ordinary, fixture.ordinary),
            };
            let before = serde_json::to_value(&fixture.app.tree).unwrap();
            let point = position(&mut fixture.app, row);
            mouse(
                &mut fixture.app,
                point,
                MouseEventKind::Down(MouseButton::Right),
            );
            let Mode::ContextMenu(menu) = &fixture.app.mode else {
                panic!("right click must open a context menu");
            };
            assert_eq!(menu.target, expected, "{kind}");
            assert!(menu.actions.contains(&action));
            fixture.app.execute_context_action(action, expected);
            match action {
                ContextMenuAction::Rename => {
                    key(&mut fixture.app, KeyCode::Enter);
                    assert!(fixture.app.take_outbound_requests().iter().any(|request| matches!(request, ClientRequest::RenameNode { node_id, .. } if *node_id == expected)));
                }
                ContextMenuAction::Close if expected == fixture.project => {
                    assert!(
                        matches!(fixture.app.mode, Mode::ConfirmClose(target) if target == fixture.project)
                    );
                    assert!(fixture.app.take_outbound_requests().is_empty());
                }
                ContextMenuAction::Close => {
                    assert!(fixture.app.take_outbound_requests().iter().any(|request| matches!(request, ClientRequest::ClosePane { pane_id } if *pane_id == expected)));
                }
                ContextMenuAction::Search => {
                    assert!(matches!(fixture.app.mode, Mode::Search(_)));
                    assert!(fixture.app.take_outbound_requests().is_empty());
                }
                _ => unreachable!(),
            }
            assert_eq!(serde_json::to_value(&fixture.app.tree).unwrap(), before);
        }
    }
}

#[test]
fn keyboard_rename_and_close_resolve_verified_rows_and_ignore_unowned_virtual_rows() {
    for kind in ["local", "ordinary", "resource", "reference"] {
        let mut fixture = fixture();
        let (row, expected) = match kind {
            "local" => (fixture.local_row, Some(fixture.local)),
            "ordinary" => (fixture.ordinary, Some(fixture.ordinary)),
            "resource" => (fixture.resource, None),
            _ => (fixture.reference, None),
        };
        let path = if row == fixture.ordinary || row == fixture.resource {
            vec![fixture.project, row]
        } else {
            vec![fixture.project, fixture.resource, row]
        };
        fixture.app.select_tree_path(path);
        fixture.app.action_start_rename();
        key(&mut fixture.app, KeyCode::Enter);
        let renamed: Vec<_> = fixture
            .app
            .take_outbound_requests()
            .into_iter()
            .filter_map(|request| {
                if let ClientRequest::RenameNode { node_id, .. } = request {
                    Some(node_id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            renamed,
            expected.into_iter().collect::<Vec<_>>(),
            "{kind} rename"
        );
        fixture.app.mode = Mode::Normal;
        fixture.app.action_close_selected();
        let closed: Vec<_> = fixture
            .app
            .take_outbound_requests()
            .into_iter()
            .filter_map(|request| {
                if let ClientRequest::ClosePane { pane_id } = request {
                    Some(pane_id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            closed,
            expected.into_iter().collect::<Vec<_>>(),
            "{kind} close"
        );
    }
}

#[test]
fn virtual_rows_cannot_start_drags_or_accept_real_pane_drops() {
    for (kind, refresh_before_release) in [
        ("resource", false),
        ("reference", false),
        ("local", false),
        ("resource", true),
        ("reference", true),
        ("local", true),
    ] {
        let mut fixture = fixture();
        let before = serde_json::to_value(&fixture.app.tree).unwrap();
        let row = match kind {
            "resource" => fixture.resource,
            "reference" => fixture.reference,
            _ => fixture.local_row,
        };
        let virtual_point = position(&mut fixture.app, row);
        mouse(
            &mut fixture.app,
            virtual_point,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert_eq!(
            fixture.app.drag_source(),
            None,
            "{kind} is a projection, not a movable node"
        );
        fixture.app.focus = FocusTarget::Tree;
        let ordinary_point = position(&mut fixture.app, fixture.ordinary);
        mouse(
            &mut fixture.app,
            ordinary_point,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert_eq!(
            fixture.app.drag_source(),
            Some(fixture.ordinary),
            "{kind}, refresh_before_release={refresh_before_release}"
        );
        render(&mut fixture.app);
        let virtual_point = position(&mut fixture.app, row);
        mouse(
            &mut fixture.app,
            virtual_point,
            MouseEventKind::Drag(MouseButton::Left),
        );
        // Exercise both the stale-frame cancellation and the current-frame
        // structural drop rejection, rather than relying on either alone.
        if refresh_before_release {
            render(&mut fixture.app);
            assert!(fixture.app.pointer_geometry_is_current());
        } else {
            // A host resize invalidates the destination until its frame is
            // acknowledged. The release must still cancel the owned drag.
            fixture
                .app
                .set_screen_area(ratatui::layout::Rect::new(0, 0, 121, 40));
            assert!(!fixture.app.pointer_geometry_is_current());
        }
        mouse(
            &mut fixture.app,
            virtual_point,
            MouseEventKind::Up(MouseButton::Left),
        );
        assert_eq!(fixture.app.drag_source(), None);
        assert!(!fixture.app.is_tree_drag_in_progress());
        assert!(!fixture
            .app
            .take_outbound_requests()
            .iter()
            .any(|request| matches!(
                request,
                ClientRequest::ReparentNode { .. } | ClientRequest::NewPane { .. }
            )));
        assert_eq!(serde_json::to_value(&fixture.app.tree).unwrap(), before);
    }
}

#[test]
fn resource_and_initial_tree_snapshots_commute_without_resetting_observation_clock() {
    let (tree, catalog, project, pane) = tree::tests::fixture();
    assert_eq!(project, NodeId(1));
    assert_eq!(pane, NodeId(2));
    let expected_tree = serde_json::to_value(&tree).unwrap();
    let clock = ResourceClock {
        unix_millis: 123_456,
        monotonic_millis: 54_321,
        boot_id: "snapshot-order-boot".into(),
    };
    let resource_event = ilium_ipc::ServerEvent::ResourceSnapshot {
        local_invocations: vec![catalog.resources[&ResourceId::from("gpu")].reservations[0]
            .agent
            .clone()],
        catalog,
        clock: clock.clone(),
        active_waits: vec![(NodeId(2), ReservationId::from("reservation"))],
        wait_revision: 7,
    };
    let tree_event = ilium_ipc::ServerEvent::PaneStateSnapshot {
        tree,
        detection_evidence: Vec::new(),
    };
    for resource_first in [true, false] {
        let mut app = App::new("test".into(), "/synthetic-resource-project".into());
        if !resource_first {
            crate::render_cache::apply(&mut app, tree_event.clone());
        }
        crate::render_cache::apply(&mut app, resource_event.clone());
        let observed_at = app.resources.observed_at;
        assert!(observed_at.is_some());
        // Replacing the tree must rebuild projections even when no newer
        // catalog revision will arrive, and must not make its clock younger.
        crate::render_cache::apply(&mut app, tree_event.clone());
        assert_eq!(app.resources.observed_at, observed_at);
        assert_eq!(app.resources.clock.as_ref(), Some(&clock));
        assert_eq!(app.resources.wait_revision, Some(7));
        assert_eq!(
            app.resources.waits.get(&NodeId(2)),
            Some(&ReservationId::from("reservation"))
        );
        let targets: Vec<_> = app.resources.rows.targets.values().cloned().collect();
        assert_eq!(targets.len(), 2, "resource_first={resource_first}");
        assert!(targets.contains(&ResourceRowTarget::Resource {
            project_id: NodeId(1),
            resource_id: ResourceId::from("gpu"),
        }));
        assert!(targets.contains(&ResourceRowTarget::Participant {
            project_id: NodeId(1),
            resource_id: ResourceId::from("gpu"),
            reservation_id: ReservationId::from("reservation"),
            pane_id: Some(NodeId(2)),
        }));
        assert_eq!(serde_json::to_value(&app.tree).unwrap(), expected_tree);
    }
}
