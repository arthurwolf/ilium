//! Resource control panel: structured layout, hit map and rendering.
//!
//! `layout` is pure geometry shared by rendering, mouse hit-testing and hover
//! tooltips, so what is drawn is exactly what can be clicked.
use super::{
    detail::{participant_color, segment_spans},
    panel_actions::{ActionGroup, ResourceAction},
    presentation,
    tree::ordered,
};
use crate::app::{App, FocusTarget, RightPanelTarget};
use ilium_core::resource_queue::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

const NARROW: u16 = 60;
const DAY_MILLIS: i64 = 86_400_000;

fn safe(text: &str) -> String {
    super::terminal_text(text)
}

fn timestamp(millis: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .map_or("unknown".into(), |time| {
            time.format("%Y-%m-%d %H:%M:%S UTC").to_string()
        })
}

#[derive(Debug, Clone)]
pub(super) struct Button {
    pub action: ResourceAction,
    pub rect: Rect,
    pub enabled: Result<(), String>,
}

#[derive(Debug, Clone)]
pub(super) struct PanelLayout {
    pub header: Rect,
    pub warning: Option<Rect>,
    pub cards: Rect,
    pub queue: Rect,
    pub trend: Option<Rect>,
    pub detail: Rect,
    pub message: Option<Rect>,
    pub captions: Vec<(Rect, &'static str)>,
    pub buttons: Vec<Button>,
}

impl PanelLayout {
    /// Rectangle of the `index`-th queue table row (after bar and header).
    pub fn queue_row(&self, index: usize) -> Option<Rect> {
        let inner = Block::bordered().inner(self.queue);
        let y = inner
            .y
            .checked_add(2)?
            .checked_add(u16::try_from(index).ok()?)?;
        (y < inner.bottom()).then(|| Rect::new(inner.x, y, inner.width, 1))
    }
    pub fn button_at(&self, column: u16, row: u16) -> Option<&Button> {
        let point = ratatui::layout::Position::new(column, row);
        self.buttons
            .iter()
            .find(|button| button.rect.contains(point))
    }
}

fn button_text(action: ResourceAction, record: &ResourceRecord) -> String {
    format!("[{}]", action.label(record))
}

/// Flows every group of buttons into rows of `width` cells starting at
/// `origin`, returning captions, buttons and the number of rows used.
fn flow(
    app: &App,
    record: &ResourceRecord,
    origin: Rect,
) -> (Vec<(Rect, &'static str)>, Vec<Button>, u16) {
    let mut captions = Vec::new();
    let mut buttons = Vec::new();
    let (mut x, mut y) = (origin.x, origin.y);
    let right = origin.right();
    for group in [
        ActionGroup::Resource,
        ActionGroup::Queue,
        ActionGroup::Recovery,
        ActionGroup::Remove,
    ] {
        let caption = group.caption();
        let caption_width = u16::try_from(caption.width() + 1).unwrap_or(u16::MAX);
        let members: Vec<_> = ResourceAction::ALL
            .into_iter()
            .filter(|action| action.group() == group)
            .collect();
        let first_width = members
            .first()
            .map_or(0, |action| button_text(*action, record).width() as u16);
        if x > origin.x && x.saturating_add(caption_width + first_width) > right {
            x = origin.x;
            y = y.saturating_add(1);
        }
        if y < origin.bottom() {
            captions.push((Rect::new(x, y, caption_width.min(right - x), 1), caption));
        }
        x = x.saturating_add(caption_width);
        for action in members {
            let text = button_text(action, record);
            let width = u16::try_from(text.width())
                .unwrap_or(u16::MAX)
                .min(origin.width);
            if x > origin.x && x.saturating_add(width) > right {
                x = origin.x;
                y = y.saturating_add(1);
            }
            if y < origin.bottom() {
                buttons.push(Button {
                    action,
                    rect: Rect::new(x, y, width, 1),
                    enabled: action.availability(app, record),
                });
            }
            x = x.saturating_add(width + 1);
        }
        x = x.saturating_add(1);
    }
    (captions, buttons, y.saturating_sub(origin.y) + 1)
}

pub(super) fn layout(app: &App, record: &ResourceRecord, area: Rect) -> PanelLayout {
    let inner = Block::bordered().inner(area);
    let (_, _, rows) = flow(
        app,
        record,
        Rect::new(inner.x, inner.y, inner.width, u16::MAX / 2),
    );
    let has_warning = warning_text(app, record).is_some();
    let queue_len = ordered(record).len();
    let queue_height = if queue_len == 0 {
        3
    } else {
        4 + u16::try_from(queue_len.min(6)).unwrap_or(6)
    };
    let show_trend = inner.height >= 30 && inner.width >= NARROW;
    let has_message = app.resources.error.is_some() || app.resources.pending.is_some();
    let [header, warning, cards, queue, trend, detail, message, buttons] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(u16::from(has_warning)),
        Constraint::Length(if inner.width >= NARROW { 3 } else { 1 }),
        Constraint::Length(queue_height),
        Constraint::Length(if show_trend { 4 } else { 0 }),
        Constraint::Fill(1),
        Constraint::Length(u16::from(has_message)),
        Constraint::Length(rows),
    ])
    .areas(inner);
    let (captions, flowed, _) = flow(app, record, buttons);
    PanelLayout {
        header,
        warning: has_warning.then_some(warning),
        cards,
        queue,
        trend: show_trend.then_some(trend),
        detail,
        message: has_message.then_some(message),
        captions,
        buttons: flowed,
    }
}

fn warning_text(app: &App, record: &ResourceRecord) -> Option<String> {
    let debt = app.resources.catalog.as_ref().is_some_and(|catalog| {
        catalog
            .links
            .iter()
            .any(|link| link.resource_id == record.definition.id && link.sync_debt.is_some())
    });
    let clock = app.resources.clock_now();
    let presentation = presentation::project(
        record,
        clock.as_ref(),
        app.resources.observation_error.as_deref(),
    );
    if let Some(error) = &app.resources.observation_error {
        Some(format!(
            "⚠ Observation failed · ETA unknown · {}",
            safe(error)
        ))
    } else if record.clearance_required {
        Some(
            "⚠ Physical clearance required · press [Confirm safe] once the hardware is safe".into(),
        )
    } else if let Some(reason) = presentation.uncertainty.as_ref() {
        Some(format!("⚠ ETA unknown · {}", safe(reason)))
    } else if debt {
        Some("⚠ Instruction sync needs repair · press [Repair setup]".into())
    } else {
        None
    }
}

fn elapsed_seconds(app: &App, reservation: &Reservation) -> Option<u64> {
    let clock = app.resources.clock_now()?;
    let acquired = reservation.acquired_at.as_ref()?;
    (clock.boot_id == acquired.boot_id)
        .then(|| {
            clock
                .monotonic_millis
                .checked_sub(acquired.monotonic_millis)
        })
        .flatten()
        .map(|millis| millis / 1000)
}

fn busy_ratio(record: &ResourceRecord, now: i64) -> f64 {
    let start = now - DAY_MILLIS;
    let busy: i64 = record
        .reservations
        .iter()
        .filter_map(|claim| {
            let acquired = claim.acquired_at.as_ref()?.unix_millis;
            let end = claim.terminal_at.unwrap_or(now).min(now);
            let begin = acquired.max(start);
            (end > begin).then_some(end - begin)
        })
        .sum();
    (busy as f64 / DAY_MILLIS as f64).clamp(0.0, 1.0)
}

fn hold_seconds(app: &App, record: &ResourceRecord) -> Vec<u64> {
    let mut finished: Vec<_> = record
        .reservations
        .iter()
        .filter_map(|claim| {
            let acquired = claim.acquired_at.as_ref()?.unix_millis;
            let end = claim.terminal_at?;
            Some((end, u64::try_from((end - acquired).max(0) / 1000).ok()?))
        })
        .collect();
    finished.sort_by_key(|(end, _)| *end);
    let mut seconds: Vec<u64> = finished.into_iter().map(|(_, hold)| hold).collect();
    if let Some(owner) = record
        .reservations
        .iter()
        .find(|claim| claim.state.occupied())
    {
        seconds.push(elapsed_seconds(app, owner).unwrap_or(0));
    }
    seconds
}

fn plain_kind(kind: &ResourceLifecycleKind) -> &'static str {
    match kind {
        ResourceLifecycleKind::Created => "Created",
        ResourceLifecycleKind::Linked => "Added to a project",
        ResourceLifecycleKind::Unlinked => "Removed from a project",
        ResourceLifecycleKind::Edited => "Edited",
        ResourceLifecycleKind::Archived => "Deleted",
        ResourceLifecycleKind::Reserved => "Joined the queue",
        ResourceLifecycleKind::Acquired => "Took the hardware",
        ResourceLifecycleKind::Released => "Gave the hardware back",
        ResourceLifecycleKind::Cancelled => "Left the queue",
        ResourceLifecycleKind::Revoked => "Ownership taken back",
        ResourceLifecycleKind::NeedsAttention => "Needs attention",
        ResourceLifecycleKind::GateChanged => "Queue paused, resumed or offline",
        ResourceLifecycleKind::RecoveryConfirmed => "Hardware confirmed safe",
        ResourceLifecycleKind::EstimateChanged => "Time estimate changed",
        ResourceLifecycleKind::Acknowledged => "Acknowledged",
        ResourceLifecycleKind::ProjectRebound => "Project moved",
        ResourceLifecycleKind::BootFenced => "Cleared after a restart",
    }
}

fn kind_color(kind: &ResourceLifecycleKind) -> Color {
    match kind {
        ResourceLifecycleKind::Revoked
        | ResourceLifecycleKind::NeedsAttention
        | ResourceLifecycleKind::BootFenced
        | ResourceLifecycleKind::Archived => Color::Yellow,
        ResourceLifecycleKind::Acquired | ResourceLifecycleKind::RecoveryConfirmed => Color::Green,
        ResourceLifecycleKind::Reserved | ResourceLifecycleKind::GateChanged => Color::Cyan,
        _ => Color::Gray,
    }
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))
}

/// `label ……… value`, with grey dotted leaders when there is room.
fn row(label: &str, value: String, width: u16) -> Line<'static> {
    let gap = usize::from(width).saturating_sub(label.width() + value.width());
    if gap < 5 {
        return Line::from(format!("{label}: {value}"));
    }
    Line::from(vec![
        Span::styled(label.to_owned(), Style::default().fg(Color::Gray)),
        crate::value_control::leader_span(gap),
        Span::raw(value),
    ])
}

fn detail_lines(app: &App, record: &ResourceRecord, width: u16) -> Vec<Line<'static>> {
    let definition = &record.definition;
    let revision = app
        .resources
        .catalog
        .as_ref()
        .map_or(0, |catalog| catalog.revision);
    let mut lines = vec![heading("About this resource")];
    lines.push(row("Resource", definition.id.to_string(), width));
    lines.push(row("Kind", safe(&definition.kind), width));
    lines.push(row(
        "Hardware identity",
        definition
            .physical_identity
            .as_deref()
            .map(safe)
            .unwrap_or("Unspecified — identity is user managed".into()),
        width,
    ));
    lines.push(row("Catalog revision", revision.to_string(), width));
    lines.push(row(
        "Created",
        timestamp(definition.created_at_unix_millis),
        width,
    ));
    lines.push(row(
        "Updated",
        timestamp(definition.updated_at_unix_millis),
        width,
    ));
    lines.push(Line::from(safe(&definition.description)));
    lines.push(Line::from(format!(
        "Advisory rules: {}",
        safe(&definition.rules)
    )));
    lines.push(Line::from(
        "Enforced policy: exclusive, first in first out; estimates never release hardware.",
    ));
    match &record.gate {
        ResourceGate::Open => {}
        ResourceGate::Paused { reason } | ResourceGate::Unavailable { reason } => {
            lines.push(Line::from(Span::styled(
                safe(reason),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    lines.push(Line::default());
    lines.push(heading("Requests in detail"));
    let queue = ordered(record);
    let clock = app.resources.clock_now();
    let presentation = presentation::project(
        record,
        clock.as_ref(),
        app.resources.observation_error.as_deref(),
    );
    let snapshot = presentation.snapshot.as_ref();
    if queue.is_empty() {
        lines.push(Line::from("No current owner or waiting requests."));
    }
    for reservation in &queue {
        let entry = snapshot.and_then(|snapshot| {
            snapshot
                .queue
                .iter()
                .find(|entry| entry.reservation.id == reservation.id)
        });
        let elapsed = elapsed_seconds(app, reservation)
            .map_or("unknown".into(), |seconds| format!("{seconds}s"));
        let start = entry
            .and_then(|entry| entry.estimate.estimated_start_unix_millis)
            .or_else(|| {
                reservation
                    .acquired_at
                    .as_ref()
                    .map(|clock| clock.unix_millis)
            });
        lines.push(Line::from(Span::styled(
            format!(
                "{} {} · {}s estimate",
                if app.resources.selected_reservation.as_ref() == Some(&reservation.id) {
                    "▸"
                } else {
                    " "
                },
                safe(&reservation.agent.label),
                reservation.estimate_seconds
            ),
            Style::default().fg(participant_color(&reservation.id)),
        )));
        lines.push(Line::from(format!(
            "  {} · project {} · pane {} · generation {}",
            reservation.id,
            reservation.agent.project_id,
            reservation.agent.pane_id.0,
            reservation.ownership_generation
        )));
        lines.push(Line::from(format!(
            "  Requested {} · acquired {} · elapsed {elapsed} · estimated start {}",
            timestamp(reservation.requested_at.unix_millis),
            reservation
                .acquired_at
                .as_ref()
                .map_or("pending".into(), |clock| timestamp(clock.unix_millis)),
            start.map_or("unknown".into(), timestamp)
        )));
        lines.push(Line::from(format!(
            "  Origin session {} · runtime {} · invocation {} · local attached wait {} · handoff pending {}",
            safe(&reservation.agent.session_id),
            safe(&reservation.agent.runtime_generation),
            reservation.agent.invocation_id,
            app.resources.waits.values().any(|id| id == &reservation.id),
            reservation.acquisition_pending
        )));
        if let ReservationState::NeedsAttention { reason } = &reservation.state {
            lines.push(Line::from(Span::styled(
                format!("  {}", safe(reason)),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    lines.push(Line::default());
    lines.push(heading("Projects using it"));
    let mut any_link = false;
    if let Some(catalog) = &app.resources.catalog {
        for link in catalog
            .links
            .iter()
            .filter(|link| link.resource_id == definition.id)
        {
            any_link = true;
            lines.push(row(
                &safe(&link.project_path),
                format!(
                    "slug {} · since {}",
                    safe(&link.slug),
                    timestamp(link.created_at_unix_millis)
                ),
                width,
            ));
            if let Some(debt) = &link.sync_debt {
                lines.push(Line::from(Span::styled(
                    format!("  Instruction/settings sync pending: {}", safe(debt)),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }
    }
    if !any_link {
        lines.push(Line::from("No project uses this resource."));
    }
    lines.push(Line::default());
    lines.push(heading("Activity log · recent durable events"));
    let mut events = 0;
    if let Some(catalog) = &app.resources.catalog {
        for event in catalog
            .history
            .iter()
            .filter(|event| event.resource_id == definition.id)
            .rev()
            .take(32)
        {
            events += 1;
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", timestamp(event.unix_millis)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{} ", plain_kind(&event.kind)),
                    Style::default()
                        .fg(kind_color(&event.kind))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    "· revision {}.{} · {}",
                    event.revision,
                    event.ordinal,
                    safe(&event.message)
                )),
            ]));
        }
    }
    if events == 0 {
        lines.push(Line::from("Nothing has happened yet."));
    }
    let mut finished: Vec<_> = record
        .reservations
        .iter()
        .filter(|reservation| !reservation.state.active())
        .collect();
    finished.sort_by_key(|reservation| std::cmp::Reverse(reservation.terminal_at));
    if !finished.is_empty() {
        lines.push(Line::default());
        lines.push(heading("Finished requests (latest 16)"));
        for reservation in finished.into_iter().take(16) {
            lines.push(Line::from(format!(
                "{} · {} · {:?} · ended {}",
                reservation.id,
                safe(&reservation.agent.label),
                reservation.state,
                reservation.terminal_at.map_or("unknown".into(), timestamp)
            )));
        }
    }
    lines
}

fn queue_state_text(
    app: &App,
    reservation: &Reservation,
    snapshot: Option<&ResourceQueueSnapshot>,
) -> (String, String) {
    if matches!(reservation.state, ReservationState::NeedsAttention { .. }) {
        return ("Needs attention".into(), "—".into());
    }
    if reservation.state.occupied() {
        let held = elapsed_seconds(app, reservation)
            .map_or("held ?".into(), |seconds| format!("held {seconds}s"));
        return ("Using now".into(), held);
    }
    let entry = snapshot.and_then(|snapshot| {
        snapshot
            .queue
            .iter()
            .find(|entry| entry.reservation.id == reservation.id)
    });
    match entry {
        Some(entry) => (
            format!("Waiting · queue {}", entry.position),
            entry
                .estimate
                .wait_seconds
                .map_or("ETA unknown".into(), |seconds| format!("about {seconds}s")),
        ),
        None => ("Waiting".into(), "ETA unknown".into()),
    }
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let RightPanelTarget::Resource {
        ref resource_id, ..
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
    let focused = app.focus == FocusTarget::Pane;
    frame.render_widget(
        crate::theme::block(focused).title("Resource control panel"),
        area,
    );
    let panel = layout(app, record, area);
    render_header(app, frame, record, &panel);
    if let Some(rect) = panel.warning {
        if let Some(text) = warning_text(app, record) {
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(Color::Yellow)),
                rect,
            );
        }
    }
    render_cards(app, frame, record, &panel);
    render_queue(app, frame, record, &panel);
    if let Some(rect) = panel.trend {
        render_trend(app, frame, record, rect);
    }
    let width = panel.detail.width.saturating_sub(2);
    let lines = detail_lines(app, record, width);
    let max_scroll = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(panel.detail.height.saturating_sub(2));
    frame.render_widget(
        Paragraph::new(super::sanitized_lines(lines))
            .wrap(Wrap { trim: false })
            .scroll((app.resources.scroll.min(max_scroll), 0))
            .block(Block::bordered().title("Details and activity log · scroll with the wheel")),
        panel.detail,
    );
    if let Some(rect) = panel.message {
        let (text, color) = if let Some(error) = &app.resources.error {
            (safe(error), Color::Red)
        } else {
            (
                "Saving / loading authoritative resource state…".into(),
                Color::DarkGray,
            )
        };
        frame.render_widget(Paragraph::new(text).style(Style::default().fg(color)), rect);
    }
    for (rect, caption) in &panel.captions {
        frame.render_widget(
            Paragraph::new(format!("{caption} ")).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            *rect,
        );
    }
    for button in &panel.buttons {
        let hovered = app
            .resources
            .hovered_action
            .is_some_and(|(action, _)| action == button.action);
        let style = if button.enabled.is_err() {
            Style::default().fg(Color::DarkGray)
        } else if matches!(
            button.action,
            ResourceAction::Delete | ResourceAction::TakeBack
        ) {
            Style::default().fg(Color::LightRed)
        } else {
            Style::default().fg(Color::Cyan)
        };
        let style = if hovered {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        };
        frame.render_widget(
            Paragraph::new(button_text(button.action, record)).style(style),
            button.rect,
        );
    }
}

fn render_header(app: &App, frame: &mut Frame, record: &ResourceRecord, panel: &PanelLayout) {
    let badge_color = match record.gate {
        ResourceGate::Open if record.reservations.iter().any(|c| c.state.occupied()) => {
            Color::Yellow
        }
        ResourceGate::Open => Color::Green,
        _ => Color::LightRed,
    };
    let state = super::tree::configured_state_label(
        record,
        &app.ui_settings.icons,
        app.ui_settings.use_stable_glyphs,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    safe(&record.definition.title),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(" {state} "),
                    Style::default().fg(Color::Black).bg(badge_color),
                ),
            ]),
            Line::from(Span::styled(
                format!(
                    "{} · {}",
                    safe(&record.definition.kind),
                    record
                        .definition
                        .physical_identity
                        .as_deref()
                        .map(safe)
                        .unwrap_or("hardware identity unspecified".into())
                ),
                Style::default().fg(Color::Gray),
            )),
        ]),
        panel.header,
    );
}

fn render_cards(app: &App, frame: &mut Frame, record: &ResourceRecord, panel: &PanelLayout) {
    let owner = record.reservations.iter().find(|c| c.state.occupied());
    let waiting = record
        .reservations
        .iter()
        .filter(|c| matches!(c.state, ReservationState::Queued))
        .count();
    let state = super::tree::configured_state_label(
        record,
        &app.ui_settings.icons,
        app.ui_settings.use_stable_glyphs,
    );
    let owner_text = owner.map_or("nobody".to_owned(), |claim| {
        let held = elapsed_seconds(app, claim).map_or(String::new(), |s| format!(" · {s}s"));
        format!("{}{held}", safe(&claim.agent.label))
    });
    let now = app.resources.clock_now().map(|clock| clock.unix_millis);
    if panel.cards.height < 3 {
        frame.render_widget(
            Paragraph::new(format!("{state} · owner {owner_text} · {waiting} waiting")),
            panel.cards,
        );
        return;
    }
    let cells = Layout::horizontal([Constraint::Fill(1); 4]).split(panel.cards);
    let card = |title: &'static str| Block::bordered().title(title);
    frame.render_widget(
        Paragraph::new(state.clone()).block(card("Status")),
        cells[0],
    );
    frame.render_widget(Paragraph::new(owner_text).block(card("Held by")), cells[1]);
    frame.render_widget(
        Paragraph::new(format!("{waiting} in line")).block(card("Waiting")),
        cells[2],
    );
    let busy = card("Busy, last 24 h");
    match now {
        Some(now) => {
            let ratio = busy_ratio(record, now);
            frame.render_widget(
                Gauge::default()
                    .ratio(ratio)
                    .label(format!("{:.0}%", ratio * 100.0))
                    .gauge_style(Style::default().fg(Color::Cyan))
                    .block(busy),
                cells[3],
            );
        }
        None => frame.render_widget(Paragraph::new("unknown").block(busy), cells[3]),
    }
}

fn render_queue(app: &App, frame: &mut Frame, record: &ResourceRecord, panel: &PanelLayout) {
    let block = Block::bordered().title("Queue · first in first out");
    let inner = block.inner(panel.queue);
    frame.render_widget(block, panel.queue);
    let queue = ordered(record);
    if queue.is_empty() {
        frame.render_widget(
            Paragraph::new("Nobody holds this resource and nobody is waiting."),
            inner,
        );
        return;
    }
    let clock = app.resources.clock_now();
    let presentation = presentation::project(
        record,
        clock.as_ref(),
        app.resources.observation_error.as_deref(),
    );
    let selected = app
        .resources
        .selected_reservation
        .as_ref()
        .and_then(|id| queue.iter().position(|claim| &claim.id == id));
    if inner.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(segment_spans(
            &queue,
            &presentation,
            selected,
            inner.width,
        ))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.height < 2 {
        return;
    }
    let narrow = inner.width < NARROW;
    let header = Row::new(if narrow {
        vec!["#", "Agent", "State"]
    } else {
        vec!["#", "Agent", "State", "Estimate", "Time"]
    })
    .style(
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    let rows = queue.iter().enumerate().map(|(index, claim)| {
        let (state, time) = queue_state_text(app, claim, presentation.snapshot.as_ref());
        let color = participant_color(&claim.id);
        let mut cells = vec![
            Cell::from(Span::styled(
                format!("{}", index + 1),
                Style::default().fg(color),
            )),
            Cell::from(safe(&claim.agent.label)),
            Cell::from(state),
        ];
        if !narrow {
            cells.push(Cell::from(format!("{}s", claim.estimate_seconds)));
            cells.push(Cell::from(time));
        }
        let row = Row::new(cells);
        if Some(index) == selected {
            row.style(Style::default().add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });
    let widths: Vec<Constraint> = if narrow {
        vec![
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(16),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(18),
            Constraint::Length(9),
            Constraint::Length(14),
        ]
    };
    frame.render_widget(
        Table::new(rows, widths).header(header),
        Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1),
    );
}

fn render_trend(app: &App, frame: &mut Frame, record: &ResourceRecord, rect: Rect) {
    let data = hold_seconds(app, record);
    let block = Block::bordered().title("Recent hold times (seconds, oldest to newest)");
    if data.is_empty() {
        frame.render_widget(
            Paragraph::new("No finished use yet — the graph fills as agents release the hardware.")
                .block(block),
            rect,
        );
        return;
    }
    let max = data.iter().copied().max().unwrap_or(0);
    frame.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::default().fg(Color::Cyan))
            .block(block.title_bottom(format!("longest {max}s · {} uses", data.len()))),
        rect,
    );
}
