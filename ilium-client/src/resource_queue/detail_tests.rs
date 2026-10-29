use super::*;
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

fn fixture() -> (App, NodeId, NodeId) {
    let (tree, mut catalog, project, pane) = super::super::tree::tests::fixture();
    let record = catalog.resources.get_mut(&ResourceId::from("gpu")).unwrap();
    record.definition.title = "GPU α 界".into();
    record.definition.description = "Synthetic Unicode description 界 ".repeat(12);
    let mut owner = record.reservations[0].clone();
    owner.id = ReservationId::from("owner");
    owner.ticket = 1;
    owner.state = ReservationState::Owned;
    owner.acquired_at = Some(owner.requested_at.clone());
    owner.ownership_generation = 5;
    record.reservations.insert(0, owner);
    let proofs = record
        .reservations
        .iter()
        .map(|entry| entry.agent.clone())
        .collect();
    let mut app = App::new("test".into(), "/synthetic-resource-project".into());
    app.tree = tree;
    app.receive_resource_catalog(
        catalog,
        ResourceClock {
            unix_millis: 31_000,
            monotonic_millis: 31_000,
            boot_id: "boot".into(),
        },
        proofs,
        vec![(pane, ReservationId::from("reservation"))],
        0,
    );
    app.show_resource(project, ResourceId::from("gpu"));
    (app, project, pane)
}

fn capture(app: &App, width: u16, height: u16, pane: Option<NodeId>) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            if let Some(pane) = pane {
                render_wait(app, frame, frame.area(), pane);
            } else {
                render(app, frame, frame.area());
            }
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    (buffer.area.y..buffer.area.bottom())
        .map(|y| {
            (buffer.area.x..buffer.area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rendered_state_matrix_keeps_controls_and_uncertainty_visible() {
    for (width, height) in [(40, 12), (80, 24), (120, 40)] {
        for state in [
            "free",
            "owned",
            "long",
            "overdue",
            "unavailable",
            "attention",
            "stale",
        ] {
            let (mut app, _, pane) = fixture();
            let record = app
                .resources
                .catalog
                .as_mut()
                .unwrap()
                .resources
                .get_mut(&ResourceId::from("gpu"))
                .unwrap();
            match state {
                "free" => record.reservations.clear(),
                "long" => {
                    let base = record.reservations[1].clone();
                    for ticket in 3..40 {
                        let mut entry = base.clone();
                        entry.id = ReservationId(format!("queued-{ticket}"));
                        entry.ticket = ticket;
                        record.reservations.push(entry);
                    }
                }
                "overdue" => record.reservations[0].estimate_seconds = 1,
                "unavailable" => {
                    record.gate = ResourceGate::Unavailable {
                        reason: "USB unplugged".into(),
                    }
                }
                "attention" => {
                    record.reservations[0].state = ReservationState::NeedsAttention {
                        reason: "owner disappeared".into(),
                    }
                }
                "stale" => app.resources.observation_error = Some("catalog unreadable".into()),
                _ => {}
            }
            let rendered = capture(&app, width, height, None);
            let visible = text(&rendered);
            assert!(
                visible.contains("[? Help]"),
                "{state} at {width}x{height}: {visible}"
            );
            if state == "free" {
                assert!(visible.contains("Free"));
            } else if state == "stale" {
                assert!(visible.contains("Observation failed"));
            } else if matches!(state, "overdue" | "unavailable" | "attention") {
                assert!(visible.contains("ETA unknown"), "{state}: {visible}");
            } else {
                assert!(visible.contains("Taken"));
            }
            // Semantic labels survive a terminal discarding all colour information.
            let mut monochrome = rendered.clone();
            for cell in &mut monochrome.content {
                cell.set_fg(Color::Gray).set_bg(Color::Black);
            }
            assert_eq!(text(&monochrome), visible);
            if state != "free" {
                let strip = text(&capture(&app, width, 2, Some(pane)));
                if matches!(state, "overdue" | "unavailable" | "attention" | "stale") {
                    assert!(strip.contains("ETA unknown"), "{state}: {strip}");
                } else {
                    assert!(strip.contains("queue position 1"));
                }
            }
        }
    }
}

#[test]
fn detail_and_wait_segments_share_elapsed_weights_and_unknown_hatching() {
    let (app, _, _) = fixture();
    let record = app.resources.record(&ResourceId::from("gpu")).unwrap();
    let clock = app.resources.clock.as_ref().unwrap();
    let model = presentation::project(record, Some(clock), None);
    assert_eq!(model.weights, vec![30, 60]);
    assert!(model.uncertainty.is_none());
    let queue = ordered(record);
    let selected = segment_spans(&queue, &model, Some(1), 1);
    assert_eq!(
        selected
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "▸"
    );
    let uncertain = presentation::project(record, Some(clock), Some("unreadable"));
    assert_eq!(uncertain.uncertainty.as_deref(), Some("unreadable"));
    assert!(segment_spans(&queue, &uncertain, Some(1), 40)
        .iter()
        .any(|span| span.content.contains('░')));
    let mut wrong_boot = clock.clone();
    wrong_boot.boot_id = "replacement boot".into();
    assert!(presentation::project(record, Some(&wrong_boot), None)
        .uncertainty
        .is_some());
}

#[test]
fn one_row_native_footer_keeps_a_selected_segment_beside_the_queue_label() {
    let (app, _, pane) = fixture();
    let buffer = capture(&app, 80, 1, Some(pane));
    assert!(text(&buffer).contains("queue position 1"));
    assert!(text(&buffer).contains('▸'));
    assert!(buffer
        .content
        .iter()
        .any(|cell| cell.bg == participant_color(&ReservationId::from("reservation"))));
}

#[test]
fn action_hit_regions_are_bounded_disjoint_and_open_contextual_help() {
    for (width, height) in [(40, 12), (80, 24), (120, 40)] {
        let (mut app, _, _) = fixture();
        let area = Rect::new(3, 2, width, height);
        app.layout.pane_area = area;
        let regions = action_regions(area);
        assert_eq!(regions.len(), 16);
        for (index, (_, label, rect)) in regions.iter().enumerate() {
            assert!(area.contains(ratatui::layout::Position::new(rect.x, rect.y)));
            assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
            assert_eq!(usize::from(rect.width), label.width());
            for (_, _, other) in &regions[index + 1..] {
                assert!(rect.intersection(*other).is_empty());
            }
        }
        let help = regions[0].2;
        assert!(handle_event(
            &mut app,
            &Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: help.x,
                row: help.y,
                modifiers: KeyModifiers::NONE,
            })
        ));
        assert!(app.resources.help_open);
        assert!(super::super::help::handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        ));
        assert!(!app.resources.help_open);
        assert!(matches!(
            app.right_panel_target,
            RightPanelTarget::Resource { .. }
        ));
    }
}

#[test]
fn split_geometry_and_real_parents_survive_wait_attach_grouping_and_detach() {
    let (mut app, project, first) = fixture();
    let second = app
        .tree
        .add_pane(project, "second", ilium_core::PaneContentKind::Terminal)
        .unwrap();
    let split = app
        .tree
        .create_split_view(
            project,
            "split",
            ilium_core::SplitOrientation::Vertical,
            &[first, second],
        )
        .unwrap();
    app.focus_pane(first);
    app.layout.pane_area = Rect::new(30, 1, 90, 39);
    app.ui_settings.progress_monitor_enabled = true;
    app.resources.waits.clear();
    let before = app.pane_viewports();
    let tree = serde_json::to_vec(&app.tree).unwrap();
    assert_eq!(before.len(), 2);
    app.resources
        .waits
        .insert(first, ReservationId::from("reservation"));
    app.ui_settings.resource_queue_grouping = false;
    let waiting = app.pane_viewports();
    assert_eq!(waiting.len(), before.len());
    for (original, current) in before.iter().zip(&waiting) {
        assert_eq!(original.pane_id, current.pane_id);
        assert_eq!(original.outer_area, current.outer_area);
        assert_eq!(original.slot_index, current.slot_index);
    }
    // The native wait footer uses space inside its pane, never a sibling's split.
    assert!(waiting[0].progress_area.is_some());
    assert!(waiting[0].content_area.height < before[0].content_area.height);
    assert_eq!(waiting[1], before[1]);
    app.ui_settings.resource_queue_grouping = true;
    assert_eq!(waiting, app.pane_viewports());
    app.resources.waits.clear();
    assert_eq!(before, app.pane_viewports());
    assert_eq!(app.tree.parent_of(first), Some(split));
    assert_eq!(app.tree.parent_of(second), Some(split));
    assert_eq!(serde_json::to_vec(&app.tree).unwrap(), tree);
    app.show_resource(project, ResourceId::from("gpu"));
    assert!(handle_event(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    ));
    assert_eq!(before, app.pane_viewports());
    app.focus_pane(second);
    assert_eq!(app.active_pane_id(), Some(second));
}

#[test]
fn queue_churn_preserves_selection_and_participant_colour_by_reservation_identity() {
    let (mut app, _, _) = fixture();
    let selected = ReservationId::from("reservation");
    app.resources.selected_reservation = Some(selected.clone());
    let colour = participant_color(&selected);
    let mut catalog = app.resources.catalog.clone().unwrap();
    catalog.revision += 1;
    catalog
        .resources
        .get_mut(&ResourceId::from("gpu"))
        .unwrap()
        .reservations
        .reverse();
    app.receive_resource_catalog(
        catalog,
        app.resources.clock.clone().unwrap(),
        app.resources.local_invocations.clone(),
        Vec::new(),
        1,
    );
    assert_eq!(app.resources.selected_reservation, Some(selected.clone()));
    assert_eq!(participant_color(&selected), colour);
    assert_eq!(
        ordered(app.resources.record(&ResourceId::from("gpu")).unwrap())[0].id,
        ReservationId::from("owner")
    );
}

#[test]
fn occupancy_and_gate_remain_independent_semantic_labels() {
    let (mut app, _, _) = fixture();
    let record = app
        .resources
        .catalog
        .as_mut()
        .unwrap()
        .resources
        .get_mut(&ResourceId::from("gpu"))
        .unwrap();
    record.gate = ResourceGate::Unavailable {
        reason: "disconnected".into(),
    };
    let label = super::super::tree::configured_state_label(record, &app.ui_settings.icons, false);
    assert!(label.contains("Taken") && label.contains("Unavailable"));
    assert!(!label.contains("Free"));
}

#[test]
fn help_in_create_form_preserves_the_draft_and_consumes_input() {
    let (mut app, project, _) = fixture();
    app.open_resource_dialog(project, false);
    let pending_snapshot = app.resources.pending;
    assert!(
        pending_snapshot.is_some(),
        "opening a form refreshes authoritative catalog state"
    );
    app.take_outbound_requests();
    let draft = format!("{:?}", app.resources.dialog);
    assert!(super::super::dialog::handle_event(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE))
    ));
    assert!(app.resources.help_open);
    assert!(super::super::help::handle_event(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
    ));
    assert_eq!(format!("{:?}", app.resources.dialog), draft);
    assert_eq!(app.resources.pending, pending_snapshot);
    assert!(
        app.take_outbound_requests().is_empty(),
        "help must not submit or refresh the draft"
    );
}

#[test]
fn untrusted_terminal_text_is_sanitized_without_mutating_authoritative_utf8() {
    let (mut app, _, pane) = fixture();
    let hostile = "GPU α 界\u{1b}[2J\u{7}\r\t";
    let record = app
        .resources
        .catalog
        .as_mut()
        .unwrap()
        .resources
        .get_mut(&ResourceId::from("gpu"))
        .unwrap();
    record.definition.title = hostile.into();
    record.definition.description = hostile.into();
    record.definition.rules = hostile.into();
    record.reservations[0].agent.label = hostile.into();
    record.gate = ResourceGate::Unavailable {
        reason: hostile.into(),
    };
    let rendered = text(&capture(&app, 120, 40, None));
    assert!(rendered.contains("GPU α 界"));
    assert!(!rendered
        .chars()
        .any(|character| character.is_control() && character != '\n'));
    let strip = text(&capture(&app, 80, 2, Some(pane)));
    assert!(!strip.contains('\u{1b}'));
    assert_eq!(
        app.resources
            .record(&ResourceId::from("gpu"))
            .unwrap()
            .definition
            .title,
        hostile
    );
    let lines = super::super::sanitized_lines(vec![Line::from(hostile.to_owned())]);
    assert!(!lines[0].spans[0].content.chars().any(char::is_control));
}
