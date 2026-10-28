use super::{dialog::ResourceDialog, presentation, segments::layout_queue_segments, tree::ordered};
use crate::app::{App, FocusTarget, RightPanelTarget};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ilium_core::{resource_queue::*, NodeId};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;
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
fn timestamp(millis: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map_or("unknown".into(), |time| {
            time.format("%Y-%m-%d %H:%M:%S UTC").to_string()
        })
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
                for (key, label, rect) in action_regions(area) {
                    let _ = label;
                    if rect.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
                        return handle_event(
                            app,
                            &Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)),
                        );
                    }
                }
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
    if app.resources.pending.is_some()
        && matches!(
            key.code,
            KeyCode::Char('e' | 'p' | 'u' | 'r' | 'x' | 'c' | 'a' | 'l' | 'i' | 'd' | 'z' | 'o')
        )
    {
        app.status_message=Some("A resource request is pending; wait for its authoritative result before another management action".into());
        return true;
    }
    match key.code {
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
                let next = if key.code == KeyCode::Down {
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
                match key.code {
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
        KeyCode::Char('a') => {
            app.resources.dialog = Some(ResourceDialog::confirm(
                project_id,
                ResourceCommand::Archive { resource_id },
                None,
            ))
        }
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
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let RightPanelTarget::Resource {
        project_id,
        ref resource_id,
    } = app.right_panel_target
    else {
        return;
    };
    let Some(record) = app.resources.record(resource_id) else {
        frame.render_widget(
            Paragraph::new("Resource state unavailable; reconnect to refresh")
                .block(crate::theme::block(true).title("Shared resource")),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "{} · {}",
                state_label(app, record),
                safe(&record.definition.title)
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Resource: {} · kind: {} · revision: {}",
            record.definition.id,
            safe(&record.definition.kind),
            app.resources
                .catalog
                .as_ref()
                .map_or(0, |catalog| catalog.revision)
        )),
        Line::from(format!(
            "Physical identity: {}",
            record
                .definition
                .physical_identity
                .as_deref()
                .map(safe)
                .unwrap_or("Unspecified — identity is user managed".into())
        )),
        Line::from(safe(&record.definition.description)),
        Line::from(format!(
            "Created: {} · updated: {}",
            timestamp(record.definition.created_at_unix_millis),
            timestamp(record.definition.updated_at_unix_millis)
        )),
        Line::from(format!(
            "Advisory rules: {}",
            safe(&record.definition.rules)
        )),
        Line::from(
            "Enforced policy: exclusive, first in first out; estimates never release hardware.",
        ),
    ];
    match &record.gate {
        ResourceGate::Open => {}
        ResourceGate::Paused { reason } | ResourceGate::Unavailable { reason } => {
            lines.push(Line::from(Span::styled(
                safe(reason),
                Style::default().fg(Color::Yellow),
            )))
        }
    }
    if let Some(error) = &app.resources.observation_error {
        lines.push(Line::from(Span::styled(
            format!(
                "⚠ Observation failed; last known queue only: {}",
                safe(error)
            ),
            Style::default().fg(Color::Yellow),
        )));
    }
    let queue = ordered(record);
    let clock = app.resources.clock_now();
    let presentation = presentation::project(
        record,
        clock.as_ref(),
        app.resources.observation_error.as_deref(),
    );
    let snapshot = presentation.snapshot.as_ref();
    if !queue.is_empty() {
        let selected = app
            .resources
            .selected_reservation
            .as_ref()
            .and_then(|id| queue.iter().position(|reservation| &reservation.id == id));
        let spans = segment_spans(
            &queue,
            &presentation,
            selected,
            area.width.saturating_sub(2),
        );
        lines.push(Line::from(
            "Queue estimate segments · ▸ selected · ░ uncertain · +N overflow",
        ));
        lines.push(Line::from(spans));
        lines.push(Line::from(
            queue
                .iter()
                .enumerate()
                .map(|(index, reservation)| {
                    Span::styled(
                        format!(" {} {} ", index + 1, safe(&reservation.agent.label)),
                        Style::default().fg(participant_color(&reservation.id)),
                    )
                })
                .collect::<Vec<_>>(),
        ));
    }
    if record.clearance_required {
        lines.push(Line::from(Span::styled(
            "⚠ Physical clearance is required before another grant",
            Style::default().fg(Color::Yellow),
        )));
    }
    if queue.is_empty() {
        lines.push(Line::from("No current owner or waiting reservations."));
    }
    for reservation in queue {
        let label = if reservation.state.occupied() {
            if matches!(reservation.state, ReservationState::NeedsAttention { .. }) {
                format!(
                    "{} Needs attention",
                    icon(app, crate::icon_settings::IconTarget::ResourceAttention)
                )
            } else {
                format!(
                    "{} Owner",
                    icon(app, crate::icon_settings::IconTarget::ResourceOwner)
                )
            }
        } else {
            let entry = snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .queue
                    .iter()
                    .find(|entry| entry.reservation.id == reservation.id)
            });
            match entry {
                Some(entry) => format!(
                    "{} Queue {} · ETA {}",
                    icon(app, crate::icon_settings::IconTarget::ResourceQueued),
                    entry.position,
                    entry
                        .estimate
                        .wait_seconds
                        .map(|seconds| format!("about {seconds}s"))
                        .unwrap_or_else(|| format!(
                            "unknown ({})",
                            entry
                                .estimate
                                .reason
                                .as_deref()
                                .unwrap_or("observation unavailable")
                        ))
                ),
                None => format!(
                    "{} Queued · ETA unknown",
                    icon(app, crate::icon_settings::IconTarget::ResourceQueued)
                ),
            }
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{} {label}",
                    if app.resources.selected_reservation.as_ref() == Some(&reservation.id) {
                        "▸"
                    } else {
                        " "
                    }
                ),
                Style::default().fg(participant_color(&reservation.id)),
            ),
            Span::raw(format!(
                " · {} · {}s estimate",
                safe(&reservation.agent.label),
                reservation.estimate_seconds
            )),
        ]));
        lines.push(Line::from(format!(
            "  {} · project {} · pane {} · generation {}",
            reservation.id,
            reservation.agent.project_id,
            reservation.agent.pane_id.0,
            reservation.ownership_generation
        )));
        let elapsed = app
            .resources
            .clock_now()
            .zip(reservation.acquired_at.as_ref())
            .and_then(|(clock, acquired)| {
                (clock.boot_id == acquired.boot_id)
                    .then(|| {
                        clock
                            .monotonic_millis
                            .checked_sub(acquired.monotonic_millis)
                    })
                    .flatten()
            })
            .map(|millis| format!("{}s", millis / 1000))
            .unwrap_or("unknown".into());
        let start = snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .queue
                    .iter()
                    .find(|entry| entry.reservation.id == reservation.id)
            })
            .and_then(|entry| entry.estimate.estimated_start_unix_millis)
            .or_else(|| {
                reservation
                    .acquired_at
                    .as_ref()
                    .map(|clock| clock.unix_millis)
            });
        lines.push(Line::from(format!(
            "  Requested {} · acquired {} · elapsed {elapsed} · estimated start {}",
            timestamp(reservation.requested_at.unix_millis),
            reservation
                .acquired_at
                .as_ref()
                .map_or("pending".into(), |clock| timestamp(clock.unix_millis)),
            start.map_or("unknown".into(), timestamp)
        )));
        lines.push(Line::from(format!("  Origin session {} · runtime {} · invocation {} · local attached wait {} · handoff pending {}",safe(&reservation.agent.session_id),safe(&reservation.agent.runtime_generation),reservation.agent.invocation_id,app.resources.waits.values().any(|id|id==&reservation.id),reservation.acquisition_pending)));
        if let ReservationState::NeedsAttention { reason } = &reservation.state {
            lines.push(Line::from(format!("  {}", safe(reason))));
        }
    }
    if let Some(catalog) = &app.resources.catalog {
        let history: Vec<_> = catalog
            .history
            .iter()
            .filter(|event| &event.resource_id == resource_id)
            .rev()
            .take(32)
            .collect();
        if !history.is_empty() {
            lines.push(Line::from("Recent durable resource lifecycle events"));
            for event in history {
                lines.push(Line::from(format!(
                    "{} · revision {}.{} · {:?} · {}",
                    timestamp(event.unix_millis),
                    event.revision,
                    event.ordinal,
                    event.kind,
                    safe(&event.message)
                )));
            }
        }
    }
    let mut history: Vec<_> = record
        .reservations
        .iter()
        .filter(|reservation| !reservation.state.active())
        .collect();
    history.sort_by_key(|reservation| std::cmp::Reverse(reservation.terminal_at));
    if !history.is_empty() {
        lines.push(Line::from("Recent reservation history (bounded)"));
        for reservation in history.into_iter().take(16) {
            lines.push(Line::from(format!(
                "{} · {} · {:?} · terminal {}",
                reservation.id,
                safe(&reservation.agent.label),
                reservation.state,
                reservation.terminal_at.map_or("unknown".into(), timestamp)
            )));
        }
    }
    if let Some(catalog) = &app.resources.catalog {
        for link in catalog
            .links
            .iter()
            .filter(|link| &link.resource_id == resource_id)
        {
            lines.push(Line::from(format!(
                "Linked: {} · slug {} · created {}",
                safe(&link.project_path),
                safe(&link.slug),
                timestamp(link.created_at_unix_millis)
            )));
            if let Some(debt) = &link.sync_debt {
                lines.push(Line::from(Span::styled(
                    format!("Instruction/settings sync pending: {}", safe(debt)),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }
    }
    lines.push(Line::from(
        "e edit · p pause/reopen · u unavailable · r revoke · c confirm clear",
    ));
    lines.push(Line::from("d own estimate · z own cancel · o own release · i repair instructions · t retry exact request · x cancel selected queued reservation · l unlink · a archive · g grouping · ↑/↓ select · PgUp/PgDown scroll · Esc back"));
    if let Some(error) = &app.resources.error {
        lines.push(Line::from(Span::styled(
            safe(error),
            Style::default().fg(Color::Red),
        )));
    }
    let _ = project_id;
    let debt = app.resources.catalog.as_ref().is_some_and(|catalog| {
        catalog
            .links
            .iter()
            .any(|link| &link.resource_id == resource_id && link.sync_debt.is_some())
    });
    let warning = if let Some(error) = &app.resources.observation_error {
        Some(format!(
            "⚠ Observation failed · ETA unknown · {}",
            safe(error)
        ))
    } else if record.clearance_required {
        Some("⚠ Physical clearance required · c after cleanup; ? help".into())
    } else if let Some(reason) = presentation.uncertainty.as_ref() {
        Some(format!("⚠ ETA unknown · {}", safe(reason)))
    } else if debt {
        Some("⚠ Instruction sync needs repair · i repairs this project".into())
    } else {
        None
    };
    let top = if let Some(warning) = warning.filter(|_| area.height > 0) {
        frame.render_widget(
            Paragraph::new(warning).style(Style::default().fg(Color::Yellow)),
            Rect::new(area.x, area.y, area.width, 1),
        );
        1
    } else {
        0
    };
    let body = Rect::new(
        area.x,
        area.y.saturating_add(top),
        area.width,
        area.height.saturating_sub(action_height(area.width) + top),
    );
    frame.render_widget(
        Paragraph::new(super::sanitized_lines(lines))
            .wrap(Wrap { trim: false })
            .scroll((app.resources.scroll, 0))
            .block(
                crate::theme::block(app.focus == FocusTarget::Pane).title("Shared resource queue"),
            ),
        body,
    );
    for (_, label, rect) in action_regions(area) {
        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
            rect,
        );
    }
}
fn action_height(width: u16) -> u16 {
    if width < 60 {
        6
    } else if width < 100 {
        4
    } else {
        2
    }
}
fn action_regions(area: Rect) -> Vec<(char, &'static str, Rect)> {
    let actions = [
        ('?', "[? Help]"),
        ('e', "[e Edit]"),
        ('p', "[p Gate]"),
        ('u', "[u Unavailable]"),
        ('r', "[r Revoke]"),
        ('x', "[x Cancel]"),
        ('c', "[c Clear]"),
        ('l', "[l Unlink]"),
        ('a', "[a Archive]"),
        ('g', "[g Group]"),
        ('i', "[i Repair]"),
        ('d', "[d Estimate]"),
        ('z', "[z Own cancel]"),
        ('o', "[o Release]"),
        ('t', "[t Retry]"),
        ('v', "[v Refresh]"),
    ];
    let mut x = area.x;
    let mut y = area
        .bottom()
        .saturating_sub(action_height(area.width))
        .max(area.y);
    let mut regions = Vec::new();
    for (key, label) in actions {
        let width = u16::try_from(label.width())
            .unwrap_or(u16::MAX)
            .min(area.width);
        if x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() {
            break;
        }
        regions.push((key, label, Rect::new(x, y, width, 1)));
        x = x.saturating_add(width + 1);
    }
    regions
}
fn segment_spans(
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
fn participant_color(id: &ReservationId) -> Color {
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
