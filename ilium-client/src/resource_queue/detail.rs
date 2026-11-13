use super::{dialog::ResourceDialog, presentation, segments::layout_queue_segments, tree::ordered};
use crate::app::{App, FocusTarget, RightPanelTarget};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ilium_core::{resource_queue::*, NodeId};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
fn safe(text: &str) -> String {
    super::terminal_text(text)
}
fn icon(app: &App, target: crate::icon_settings::IconTarget) -> &str {
    app.ui_settings
        .icons
        .glyph_for_display(target, app.ui_settings.use_stable_glyphs)
}
fn state_label(app: &App, record: &ResourceRecord) -> String {
    super::tree::configured_state_label(
        record,
        &app.ui_settings.icons,
        app.ui_settings.use_stable_glyphs,
    )
}
pub fn handle_event(app: &mut App, event: &Event) -> bool {
    let RightPanelTarget::Resource {
        project_id,
        ref resource_id,
    } = app.right_panel_target
    else {
        return false;
    };
    if let Event::Mouse(mouse) = event {
        let area = app.layout.pane_area;
        if !area.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
            return false;
        }
        app.focus = FocusTarget::Pane;
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                app.resources.scroll = app.resources.scroll.saturating_sub(3)
            }
            MouseEventKind::ScrollDown => {
                app.resources.scroll = app.resources.scroll.saturating_add(3)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = match app.right_panel_target.clone() {
                    RightPanelTarget::Resource { resource_id, .. } => {
                        app.resources.record(&resource_id).and_then(|record| {
                            super::panel::layout(app, record, area)
                                .button_at(mouse.column, mouse.row)
                                .cloned()
                        })
                    }
                    _ => None,
                };
                if hit.is_none() {
                    let clicked = match app.right_panel_target.clone() {
                        RightPanelTarget::Resource { resource_id, .. } => {
                            app.resources.record(&resource_id).and_then(|record| {
                                let panel = super::panel::layout(app, record, area);
                                ordered(record).into_iter().enumerate().find_map(
                                    |(index, reservation)| {
                                        let rect = panel.queue_row(index)?;
                                        rect.contains(ratatui::layout::Position::new(
                                            mouse.column,
                                            mouse.row,
                                        ))
                                        .then(|| reservation.id.clone())
                                    },
                                )
                            })
                        }
                        _ => None,
                    };
                    if clicked.is_some() {
                        app.resources.selected_reservation = clicked;
                    }
                }
                if let Some(button) = hit {
                    return match button.enabled {
                        Ok(()) => handle_event(
                            app,
                            &Event::Key(KeyEvent::new(
                                KeyCode::Char(button.action.key()),
                                KeyModifiers::NONE,
                            )),
                        ),
                        Err(reason) => {
                            app.status_message = Some(reason);
                            true
                        }
                    };
                }
            }
            MouseEventKind::Moved => {
                let hovered = match app.right_panel_target.clone() {
                    RightPanelTarget::Resource { resource_id, .. } => {
                        app.resources.record(&resource_id).and_then(|record| {
                            super::panel::layout(app, record, area)
                                .button_at(mouse.column, mouse.row)
                                .map(|button| {
                                    (
                                        button.action,
                                        ratatui::layout::Position::new(mouse.column, mouse.row),
                                    )
                                })
                        })
                    }
                    _ => None,
                };
                app.resources.hovered_action = hovered;
            }
            _ => {}
        }
        return true;
    }
    if app.focus != FocusTarget::Pane {
        return false;
    }
    let Event::Key(key) = event else {
        return false;
    };
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return true;
    }
    let resource_id = resource_id.clone();
    perform_key(app, project_id, resource_id, key.code)
}
/// Runs one management action by its key accelerator. Buttons, the context
/// menu and the keyboard all converge here.
pub(super) fn perform_key(
    app: &mut App,
    project_id: NodeId,
    resource_id: ResourceId,
    code: KeyCode,
) -> bool {
    if app.resources.pending.is_some()
        && matches!(
            code,
            KeyCode::Char('e' | 'p' | 'u' | 'r' | 'x' | 'c' | 'a' | 'l' | 'i' | 'd' | 'z' | 'o')
        )
    {
        app.status_message=Some("A resource request is pending; wait for its authoritative result before another management action".into());
        return true;
    }
    match code {
        KeyCode::F(1) | KeyCode::Char('?') => {
            app.resources.help_open = true;
            app.resources.help_scroll = 0;
        }
        KeyCode::Esc => {
            app.right_panel_target = app
                .resources
                .previous_target
                .take()
                .unwrap_or(RightPanelTarget::Empty);
            app.reconcile_right_panel_target();
        }
        KeyCode::Up | KeyCode::Down => {
            if let Some(record) = app.resources.record(&resource_id) {
                let entries = ordered(record);
                let current = app
                    .resources
                    .selected_reservation
                    .as_ref()
                    .and_then(|id| entries.iter().position(|reservation| &reservation.id == id))
                    .unwrap_or(0);
                let next = if code == KeyCode::Down {
                    (current + 1).min(entries.len().saturating_sub(1))
                } else {
                    current.saturating_sub(1)
                };
                app.resources.selected_reservation =
                    entries.get(next).map(|reservation| reservation.id.clone());
            }
        }
        KeyCode::PageUp => app.resources.scroll = app.resources.scroll.saturating_sub(10),
        KeyCode::PageDown => app.resources.scroll = app.resources.scroll.saturating_add(10),
        KeyCode::Char('t') => app.retry_resource_request(),
        KeyCode::Char('v') => {
            app.resource_request(project_id, ilium_ipc::resource::ResourceOperation::Snapshot)
        }
        KeyCode::Char('i') => app.resource_request(
            project_id,
            ilium_ipc::resource::ResourceOperation::RepairInstructions { resource_id },
        ),
        KeyCode::Char('d' | 'z' | 'o') => {
            let selected = app
                .resources
                .record(&resource_id)
                .and_then(|record| {
                    ordered(record).into_iter().find(|reservation| {
                        Some(&reservation.id) == app.resources.selected_reservation.as_ref()
                    })
                })
                .cloned();
            if let Some(reservation) =
                selected.filter(|reservation| app.verified_resource_actor(reservation).is_some())
            {
                let Some(actor_project) = app.tree.project_ancestor(reservation.agent.pane_id)
                else {
                    app.resources.error =
                        Some("The local invocation no longer belongs to an open project".into());
                    return true;
                };
                match code {
    KeyCode::Char('d')=>app.resources.dialog=Some(ResourceDialog::estimate(actor_project,&reservation)),
    KeyCode::Char('z') if matches!(reservation.state,ReservationState::Queued)=>app.resources.dialog=Some(ResourceDialog::own_action(actor_project,&reservation,ResourceCommand::Cancel{reservation_id:reservation.id.clone()})),
    KeyCode::Char('o') if matches!(reservation.state,ReservationState::Owned)=>app.resources.dialog=Some(ResourceDialog::own_action(actor_project,&reservation,ResourceCommand::Release{reservation_id:reservation.id.clone(),ownership_generation:reservation.ownership_generation})),
    _=>app.resources.error=Some("Release requires own active ownership; cancel requires an own queued reservation".into())
   }
            } else {
                app.resources.error =
                    Some("Select a reservation belonging to a verified local invocation".into());
            }
        }
        KeyCode::Char('e') => {
            if let Some(record) = app.resources.record(&resource_id) {
                app.resources.dialog = Some(ResourceDialog::edit(project_id, &record.definition));
            }
        }
        KeyCode::Char('g') => app.settings_toggle_resource_grouping(),
        KeyCode::Char('p') => {
            let gate = match app
                .resources
                .record(&resource_id)
                .map(|record| &record.gate)
            {
                Some(ResourceGate::Open) => ResourceGate::Paused {
                    reason: "Paused by user".into(),
                },
                _ => ResourceGate::Open,
            };
            app.resources.dialog = Some(ResourceDialog::confirm(
                project_id,
                ResourceCommand::SetGate { resource_id, gate },
                None,
            ));
        }
        KeyCode::Char('u') => {
            app.resources.dialog = Some(ResourceDialog::confirm(
                project_id,
                ResourceCommand::SetGate {
                    resource_id,
                    gate: ResourceGate::Unavailable {
                        reason: "Hardware marked unavailable by user".into(),
                    },
                },
                None,
            ))
        }
        KeyCode::Char('r') => {
            if let Some(owner) = app.resources.record(&resource_id).and_then(|record| {
                record
                    .reservations
                    .iter()
                    .find(|reservation| reservation.state.occupied())
            }) {
                let command = ResourceCommand::Revoke {
                    reservation_id: owner.id.clone(),
                    reason: "User revoked ownership; hardware clearance required".into(),
                };
                app.prepare_resource_recovery(project_id, resource_id, command);
            }
        }
        KeyCode::Char('x') => {
            if let Some(reservation) = app.resources.record(&resource_id).and_then(|record| {
                ordered(record).into_iter().find(|reservation| {
                    Some(&reservation.id) == app.resources.selected_reservation.as_ref()
                        && matches!(reservation.state, ReservationState::Queued)
                })
            }) {
                let command = ResourceCommand::CancelByUser {
                    reservation_id: reservation.id.clone(),
                    reason: "User cancelled this queued reservation".into(),
                };
                app.prepare_resource_recovery(project_id, resource_id, command);
            }
        }
        KeyCode::Char('c') => app.prepare_resource_recovery(
            project_id,
            resource_id.clone(),
            ResourceCommand::ConfirmClear {
                resource_id,
                reason: "User confirmed hardware is safe for the next reservation".into(),
            },
        ),
        KeyCode::Char('a') => begin_delete(app, project_id, resource_id),
        KeyCode::Char('l') => {
            if let Some(link) = app.resources.catalog.as_ref().and_then(|catalog| {
                catalog.links.iter().find(|link| {
                    link.resource_id == resource_id
                        && app
                            .tree
                            .get(project_id)
                            .and_then(|node| node.project_path())
                            .is_some_and(|path| path == std::path::Path::new(&link.project_path))
                })
            }) {
                app.resources.dialog = Some(ResourceDialog::confirm(
                    project_id,
                    ResourceCommand::Unlink {
                        project_id: link.project_id.clone(),
                        resource_id,
                    },
                    None,
                ));
            }
        }
        _ => return false,
    }
    if let Some(dialog) = &mut app.resources.dialog {
        dialog.expected_revision = app
            .resources
            .catalog
            .as_ref()
            .map(|catalog| catalog.revision);
    }
    true
}
/// Starts deleting a resource: refuses with a plain reason while it is in use
/// or shared, otherwise opens the confirmation (unlink here, then archive).
pub(super) fn begin_delete(app: &mut App, project_id: NodeId, resource_id: ResourceId) {
    let Some(record) = app.resources.record(&resource_id) else {
        return;
    };
    let active = record
        .reservations
        .iter()
        .filter(|reservation| reservation.state.active())
        .count();
    if active > 0 {
        app.status_message = Some(format!(
            "Cannot delete: {active} agent request(s) are still using or waiting for it. Cancel or release them first."
        ));
        return;
    }
    let project_path = app
        .tree
        .get(project_id)
        .and_then(|node| node.project_path())
        .map(std::path::Path::to_path_buf);
    let links: Vec<_> = app
        .resources
        .catalog
        .as_ref()
        .map(|catalog| {
            catalog
                .links
                .iter()
                .filter(|link| link.resource_id == resource_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let (own, others): (Vec<_>, Vec<_>) = links.into_iter().partition(|link| {
        project_path
            .as_deref()
            .is_some_and(|path| path == std::path::Path::new(&link.project_path))
    });
    if !others.is_empty() {
        let names = others
            .iter()
            .map(|link| link.project_path.clone())
            .collect::<Vec<_>>()
            .join(", ");
        app.status_message = Some(format!(
            "Cannot delete: also used by {names}. Use \"Remove from this project\" there first."
        ));
        return;
    }
    let mut dialog = ResourceDialog::delete(
        project_id,
        resource_id,
        own.first().map(|link| link.project_id.clone()),
    );
    dialog.expected_revision = app
        .resources
        .catalog
        .as_ref()
        .map(|catalog| catalog.revision);
    app.resources.dialog = Some(dialog);
}
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    super::panel::render(app, frame, area);
}
pub(super) fn segment_spans(
    queue: &[&Reservation],
    presentation: &presentation::QueuePresentation,
    selected: Option<usize>,
    width: u16,
) -> Vec<Span<'static>> {
    layout_queue_segments(&presentation.weights, selected, width)
        .into_iter()
        .map(|segment| {
            let is_selected = Some(segment.entry) == selected;
            let label = if is_selected && segment.cells == 1 {
                "▸".into()
            } else if segment.overflow > 0 {
                format!("+{}", segment.overflow)
            } else {
                format!(
                    "{}{}",
                    if is_selected { "▸" } else { "" },
                    segment.entry + 1
                )
            };
            let text = if presentation.uncertainty.is_some() {
                format!("{label:░<width$}", width = usize::from(segment.cells))
            } else {
                format!("{label:width$}", width = usize::from(segment.cells))
            };
            Span::styled(
                text.chars()
                    .take(usize::from(segment.cells))
                    .collect::<String>(),
                Style::default().fg(Color::Black).bg(queue
                    .get(segment.entry)
                    .map_or(Color::DarkGray, |reservation| {
                        participant_color(&reservation.id)
                    })),
            )
        })
        .collect()
}
pub(super) fn participant_color(id: &ReservationId) -> Color {
    const COLORS: [Color; 6] = [
        Color::Cyan,
        Color::Magenta,
        Color::Yellow,
        Color::Green,
        Color::Blue,
        Color::LightRed,
    ];
    let hash = id.0.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    COLORS[hash % COLORS.len()]
}
pub fn render_wait(app: &App, frame: &mut Frame, area: Rect, pane: NodeId) {
    let Some(selected) = app.resources.waits.get(&pane) else {
        return;
    };
    let Some(record) = app.resources.catalog.as_ref().and_then(|catalog| {
        catalog.resources.values().find(|record| {
            record
                .reservations
                .iter()
                .any(|reservation| &reservation.id == selected)
        })
    }) else {
        frame.render_widget(
            Paragraph::new("⌛ Resource wait · queue state unavailable · ETA unknown")
                .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    };
    if let Some(error) = &app.resources.observation_error {
        frame.render_widget(
            Paragraph::new(format!(
                "⚠ ETA unknown · {} · queue observation failed: {}",
                safe(&record.definition.title),
                safe(error)
            ))
            .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }
    let queue = ordered(record);
    let selected_index = queue
        .iter()
        .position(|reservation| &reservation.id == selected);
    let clock = app.resources.clock_now();
    let presentation = presentation::project(
        record,
        clock.as_ref(),
        app.resources.observation_error.as_deref(),
    );
    let spans = segment_spans(&queue, &presentation, selected_index, area.width);
    let position = selected_index.map_or("unknown".into(), |index| {
        if queue
            .get(index)
            .is_some_and(|reservation| reservation.state.occupied())
        {
            "acquired".into()
        } else {
            format!(
                "queue position {}",
                queue[..=index]
                    .iter()
                    .filter(|reservation| matches!(reservation.state, ReservationState::Queued))
                    .count()
            )
        }
    });
    let estimate = presentation
        .snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .queue
                .iter()
                .find(|entry| &entry.reservation.id == selected)
        })
        .map(|entry| &entry.estimate);
    let eta = estimate
        .as_ref()
        .and_then(|estimate| estimate.wait_seconds)
        .map(|seconds| format!("ETA about {seconds}s"))
        .unwrap_or_else(|| {
            format!(
                "ETA unknown: {}",
                estimate
                    .as_ref()
                    .and_then(|estimate| estimate.reason.as_deref())
                    .unwrap_or("ownership must be confirmed")
            )
        });
    let heading = if presentation.uncertainty.is_some() {
        format!(
            "⚠ ETA unknown · {} · {position}",
            safe(&record.definition.title)
        )
    } else {
        format!(
            "{} {} · {position} · {eta}",
            icon(app, crate::icon_settings::IconTarget::ResourceQueued),
            safe(&record.definition.title)
        )
    };
    if area.height == 1 && area.width >= 20 {
        // The minimum footer budget also keeps an ordinary monitor summary.
        // Put the native legend and segments side by side in its single row.
        let segment_width = (area.width / 3).max(1);
        let heading_width = area.width.saturating_sub(segment_width + 1);
        frame.render_widget(
            Paragraph::new(heading),
            Rect::new(area.x, area.y, heading_width, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(segment_spans(
                &queue,
                &presentation,
                selected_index,
                segment_width,
            ))),
            Rect::new(area.x + heading_width + 1, area.y, segment_width, 1),
        );
        return;
    }
    let mut lines = vec![Line::from(heading)];
    if area.height > 1 {
        lines.push(Line::from(spans));
    }
    if area.height > 2 {
        lines.push(Line::from(format!(
            "{} · open resource for queue / uncertain ETA details",
            state_label(app, record)
        )));
    }
    frame.render_widget(Paragraph::new(super::sanitized_lines(lines)), area);
}

#[cfg(test)]
#[path = "detail_tests.rs"]
mod tests;
