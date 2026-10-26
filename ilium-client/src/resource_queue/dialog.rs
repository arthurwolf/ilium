use crate::app::App;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ilium_core::{resource_queue::*, NodeId};
use ilium_ipc::resource::ResourceOperation;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
#[derive(Debug, Clone)]
pub enum DialogKind {
    Estimate {
        reservation_id: ReservationId,
    },
    Create,
    Link {
        selected: usize,
        query: String,
    },
    Edit(ResourceId),
    Confirm {
        command: Box<ResourceCommand>,
        token: Option<String>,
    },
}
#[derive(Debug, Clone)]
pub struct ResourceDialog {
    pub project: NodeId,
    pub kind: DialogKind,
    pub fields: Vec<String>,
    pub field: usize,
    pub submitted: bool,
    pub expected_revision: Option<u64>,
    pub selected_resource: Option<ResourceId>,
    pub dirty_at: Option<std::time::Instant>,
    pub cursor: usize,
    pub request_key: String,
    pub create_id: ResourceId,
    pub actor: Option<AgentIdentity>,
}
impl ResourceDialog {
    pub fn create(project: NodeId) -> Self {
        Self {
            project,
            kind: DialogKind::Create,
            fields: vec![
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "gpu".into(),
                String::new(),
            ],
            field: 0,
            submitted: false,
            expected_revision: None,
            selected_resource: None,
            dirty_at: None,
            cursor: usize::MAX,
            request_key: super::new_mutation_key(),
            create_id: ResourceId(uuid::Uuid::new_v4().to_string()),
            actor: None,
        }
    }
    pub fn link(project: NodeId) -> Self {
        Self {
            project,
            kind: DialogKind::Link {
                selected: 0,
                query: String::new(),
            },
            fields: vec![String::new(), String::new()],
            field: 0,
            submitted: false,
            expected_revision: None,
            selected_resource: None,
            dirty_at: None,
            cursor: usize::MAX,
            request_key: super::new_mutation_key(),
            create_id: ResourceId(uuid::Uuid::new_v4().to_string()),
            actor: None,
        }
    }
    pub fn edit(project: NodeId, definition: &ResourceDefinition) -> Self {
        Self {
            project,
            kind: DialogKind::Edit(definition.id.clone()),
            fields: vec![
                definition.title.clone(),
                String::new(),
                definition.description.clone(),
                definition.rules.clone(),
                definition.kind.clone(),
                definition.physical_identity.clone().unwrap_or_default(),
            ],
            field: 0,
            submitted: false,
            expected_revision: None,
            selected_resource: None,
            dirty_at: None,
            cursor: usize::MAX,
            request_key: super::new_mutation_key(),
            create_id: ResourceId(uuid::Uuid::new_v4().to_string()),
            actor: None,
        }
    }
    pub fn confirm(project: NodeId, command: ResourceCommand, token: Option<String>) -> Self {
        Self {
            project,
            kind: DialogKind::Confirm {
                command: Box::new(command),
                token,
            },
            fields: vec![String::new(), String::new()],
            field: 0,
            submitted: false,
            expected_revision: None,
            selected_resource: None,
            dirty_at: None,
            cursor: usize::MAX,
            request_key: super::new_mutation_key(),
            create_id: ResourceId(uuid::Uuid::new_v4().to_string()),
            actor: None,
        }
    }
    pub fn own_action(
        project: NodeId,
        reservation: &Reservation,
        command: ResourceCommand,
    ) -> Self {
        let mut dialog = Self::confirm(project, command, None);
        dialog.actor = Some(reservation.agent.clone());
        dialog
    }
    pub fn estimate(project: NodeId, reservation: &Reservation) -> Self {
        let mut dialog = Self::own_action(
            project,
            reservation,
            ResourceCommand::Estimate {
                reservation_id: reservation.id.clone(),
                estimate_seconds: reservation.estimate_seconds,
            },
        );
        dialog.kind = DialogKind::Estimate {
            reservation_id: reservation.id.clone(),
        };
        dialog.fields = vec![reservation.estimate_seconds.to_string()];
        dialog
    }
    fn labels(&self) -> &[&str] {
        match self.kind {
            DialogKind::Estimate { .. } => &["Estimated duration (seconds)"],
            DialogKind::Link { .. } => {
                &["Search existing resources", "Local slug (blank uses title)"]
            }
            DialogKind::Confirm { .. } => &["Type CONFIRM", "Reason (required for recovery)"],
            _ => &[
                "Title",
                "Local slug (create only)",
                "Description",
                "Advisory rules",
                "Kind (gpu / usb-board / generic)",
                "Physical identity (optional)",
            ],
        }
    }
}
fn slug(title: &str) -> String {
    let mut output = String::new();
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
        } else if !output.is_empty() && !output.ends_with('-') {
            output.push('-');
        }
    }
    output.trim_matches('-').to_string()
}
fn candidates(app: &App, query: &str) -> Vec<ResourceDefinition> {
    let query = query.to_lowercase();
    let Some(catalog) = &app.resources.catalog else {
        return Vec::new();
    };
    let mut definitions: Vec<_> = catalog
        .resources
        .values()
        .filter(|record| {
            !record.definition.archived
                && (record.definition.title.to_lowercase().contains(&query)
                    || record.definition.id.0.to_lowercase().contains(&query)
                    || record.definition.kind.to_lowercase().contains(&query)
                    || catalog.links.iter().any(|link| {
                        link.resource_id == record.definition.id
                            && link.project_path.to_lowercase().contains(&query)
                    }))
        })
        .map(|record| record.definition.clone())
        .collect();
    definitions.sort_by(|left, right| left.title.cmp(&right.title).then(left.id.cmp(&right.id)));
    definitions
}
fn insert(dialog: &mut ResourceDialog, text: &str) {
    for character in text.chars() {
        let value = &mut dialog.fields[dialog.field];
        if value.len() + character.len_utf8() > 16384 {
            break;
        }
        value.insert(dialog.cursor, character);
        dialog.cursor += character.len_utf8();
    }
}
fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}
fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(index, _)| cursor + index)
}
fn physical_collision(app: &App, dialog: &ResourceDialog) -> Option<ResourceId> {
    if !matches!(dialog.kind, DialogKind::Create) {
        return None;
    }
    let physical = dialog.fields.get(5)?.trim();
    if physical.is_empty() {
        return None;
    }
    app.resources
        .catalog
        .as_ref()?
        .resources
        .values()
        .find(|record| {
            !record.definition.archived
                && record
                    .definition
                    .physical_identity
                    .as_ref()
                    .is_some_and(|identity| identity.trim().eq_ignore_ascii_case(physical))
        })
        .map(|record| record.definition.id.clone())
}
pub fn handle_event(app: &mut App, event: &Event) -> bool {
    if app.resources.dialog.is_some()
        && matches!(event,Event::Key(key) if key.code==KeyCode::F(1)&&matches!(key.kind,KeyEventKind::Press|KeyEventKind::Repeat))
    {
        app.resources.help_open = true;
        app.resources.help_scroll = 0;
        return true;
    }
    let Some(mut dialog) = app.resources.dialog.take() else {
        return false;
    };
    dialog.cursor = dialog.cursor.min(dialog.fields[dialog.field].len());
    let original_fields = dialog.fields.clone();
    let mut close = false;
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
                app.resources.dialog = Some(dialog);
                app.retry_resource_request();
                return true;
            }
            if key.code == KeyCode::Esc {
                close = true;
            } else if !dialog.submitted {
                match key.code {
                    KeyCode::Char('l')
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && physical_collision(app, &dialog).is_some() =>
                    {
                        if let Some(id) = physical_collision(app, &dialog) {
                            let slug = dialog.fields[1].clone();
                            dialog.request_key = super::new_mutation_key();
                            dialog.kind = DialogKind::Link {
                                selected: 0,
                                query: String::new(),
                            };
                            dialog.fields = vec![String::new(), slug];
                            dialog.selected_resource = Some(id);
                            dialog.field = 1;
                            dialog.cursor = dialog.fields[1].len();
                            app.resources.error = None;
                        }
                    }
                    KeyCode::Tab => {
                        dialog.field = (dialog.field + 1) % dialog.fields.len();
                        dialog.cursor = dialog.fields[dialog.field].len();
                    }
                    KeyCode::BackTab => {
                        dialog.field =
                            (dialog.field + dialog.fields.len() - 1) % dialog.fields.len();
                        dialog.cursor = dialog.fields[dialog.field].len();
                    }
                    KeyCode::Left => {
                        dialog.cursor =
                            previous_boundary(&dialog.fields[dialog.field], dialog.cursor)
                    }
                    KeyCode::Right => {
                        dialog.cursor = next_boundary(&dialog.fields[dialog.field], dialog.cursor)
                    }
                    KeyCode::Home => dialog.cursor = 0,
                    KeyCode::End => dialog.cursor = dialog.fields[dialog.field].len(),
                    KeyCode::Delete => {
                        let next = next_boundary(&dialog.fields[dialog.field], dialog.cursor);
                        dialog.fields[dialog.field].replace_range(dialog.cursor..next, "");
                    }
                    KeyCode::Up => {
                        if let DialogKind::Link { selected, .. } = &mut dialog.kind {
                            *selected = selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if let DialogKind::Link { selected, query } = &mut dialog.kind {
                            *selected =
                                (*selected + 1).min(candidates(app, query).len().saturating_sub(1));
                        }
                    }
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
                        insert(&mut dialog, "\n")
                    }
                    KeyCode::Enter => {
                        if app.resources.pending.is_none() {
                            match command(app, &dialog) {
                                Ok(command) => {
                                    let token = match &dialog.kind {
                                        DialogKind::Confirm { token, .. } => token.clone(),
                                        _ => None,
                                    };
                                    app.resource_request_full(
                                        dialog.project,
                                        ResourceOperation::Mutate(command),
                                        token,
                                        dialog.expected_revision,
                                        Some(dialog.request_key.clone()),
                                        dialog.actor.as_ref().map(|actor| actor.pane_id),
                                    );
                                    dialog.submitted = app.resources.pending.is_some();
                                }
                                Err(message) => app.resources.error = Some(message),
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        let previous =
                            previous_boundary(&dialog.fields[dialog.field], dialog.cursor);
                        dialog.fields[dialog.field].replace_range(previous..dialog.cursor, "");
                        dialog.cursor = previous;
                    }
                    KeyCode::Char(character)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        insert(&mut dialog, &character.to_string())
                    }
                    _ => {}
                }
            }
        }
        Event::Mouse(mouse) if !dialog.submitted => {
            let inner = crate::theme::block(true).inner(area(app.layout.screen_area));
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                && inner.contains(ratatui::layout::Position::new(mouse.column, mouse.row))
            {
                if inner.height >= 2 && mouse.row == inner.bottom() - 2 {
                    if mouse.column < inner.x + 8 {
                        app.resources.dialog = Some(dialog);
                        return handle_event(
                            app,
                            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                        );
                    }
                    if mouse.column >= inner.x + 10 && mouse.column < inner.x + 18 {
                        close = true;
                    }
                    if mouse.column >= inner.x + 20 && physical_collision(app, &dialog).is_some() {
                        app.resources.dialog = Some(dialog);
                        return handle_event(
                            app,
                            &Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
                        );
                    }
                } else {
                    let (_, ranges) = content(app, &dialog, inner.width);
                    let row = usize::from(mouse.row - inner.y)
                        + scroll_for(&dialog, &ranges, inner.height.saturating_sub(3));
                    if let DialogKind::Link { selected, query } = &mut dialog.kind {
                        let available = candidates(app, query);
                        let first = selected.saturating_sub(2);
                        if row > 0 && row <= 5 {
                            let index = first + row - 1;
                            if let Some(definition) = available.get(index) {
                                *selected = index;
                                dialog.selected_resource = Some(definition.id.clone());
                            }
                        }
                    }
                    if let Some(index) = ranges
                        .iter()
                        .position(|(start, end)| row >= *start && row < *end)
                    {
                        dialog.field = index;
                        dialog.cursor = dialog.fields[index].len();
                    }
                }
            }
        }
        Event::Paste(text) if !dialog.submitted => insert(&mut dialog, text),
        _ => {}
    }
    if let DialogKind::Link { selected, query } = &mut dialog.kind {
        if *query != dialog.fields[0] {
            *query = dialog.fields[0].clone();
            *selected = 0;
            dialog.selected_resource = None;
        }
        let available = candidates(app, query);
        if matches!(event,Event::Key(key) if matches!(key.code,KeyCode::Up|KeyCode::Down)) {
            dialog.selected_resource = available
                .get(*selected)
                .map(|definition| definition.id.clone());
        }
        if let Some(id) = &dialog.selected_resource {
            if let Some(index) = available.iter().position(|definition| &definition.id == id) {
                *selected = index;
            }
        }
    }
    if dialog.fields != original_fields {
        dialog.request_key = super::new_mutation_key();
        if matches!(dialog.kind, DialogKind::Edit(_)) {
            dialog.dirty_at = Some(std::time::Instant::now());
        }
    }
    if !close {
        app.resources.dialog = Some(dialog);
    }
    true
}
pub(super) fn command(app: &App, dialog: &ResourceDialog) -> Result<ResourceCommand, String> {
    if let Some(actor) = &dialog.actor {
        if !app
            .resources
            .local_invocations
            .iter()
            .any(|current| current.same_invocation(actor))
        {
            return Err("The selected local invocation is no longer verified".into());
        }
        let id = match &dialog.kind {
            DialogKind::Estimate { reservation_id } => reservation_id,
            DialogKind::Confirm { command, .. } => match command.as_ref() {
                ResourceCommand::Cancel { reservation_id }
                | ResourceCommand::Release { reservation_id, .. } => reservation_id,
                _ => return Err("Unsupported local reservation action".into()),
            },
            _ => return Err("Unsupported local reservation action".into()),
        };
        let reservation = app
            .resources
            .catalog
            .as_ref()
            .and_then(|catalog| {
                catalog
                    .resources
                    .values()
                    .flat_map(|record| record.reservations.iter())
                    .find(|reservation| {
                        &reservation.id == id
                            && reservation.agent.same_invocation(actor)
                            && reservation.state.active()
                    })
            })
            .ok_or("The original reservation is no longer active for this invocation")?;
        if let DialogKind::Confirm { command, .. } = &dialog.kind {
            match command.as_ref() {
                ResourceCommand::Release {
                    ownership_generation,
                    ..
                } if !matches!(reservation.state, ReservationState::Owned)
                    || *ownership_generation != reservation.ownership_generation =>
                {
                    return Err(
                        "Ownership generation changed; reopen this reservation before release"
                            .into(),
                    )
                }
                ResourceCommand::Cancel { .. }
                    if !matches!(reservation.state, ReservationState::Queued) =>
                {
                    return Err("Only an own queued reservation can be cancelled".into())
                }
                _ => {}
            }
        }
    }
    if let DialogKind::Estimate { reservation_id } = &dialog.kind {
        let seconds = dialog.fields[0]
            .parse::<u64>()
            .map_err(|_| "Enter an estimated duration in whole seconds")?;
        if seconds == 0 {
            return Err("Estimate must be greater than zero".into());
        }
        return Ok(ResourceCommand::Estimate {
            reservation_id: reservation_id.clone(),
            estimate_seconds: seconds,
        });
    }

    if let DialogKind::Confirm { command, .. } = &dialog.kind {
        if dialog.fields[0] != "CONFIRM" {
            return Err("Type CONFIRM to apply this revision-bound action".into());
        }
        let mut command = *command.clone();
        let reason = dialog.fields[1].trim();
        match &mut command {
            ResourceCommand::Revoke { reason: target, .. }
            | ResourceCommand::CancelByUser { reason: target, .. }
            | ResourceCommand::ConfirmClear { reason: target, .. } => {
                if reason.is_empty() {
                    return Err("Enter a reason for this recovery action".into());
                }
                *target = reason.into();
            }
            ResourceCommand::SetGate {
                gate:
                    ResourceGate::Paused { reason: target }
                    | ResourceGate::Unavailable { reason: target },
                ..
            } => {
                if reason.is_empty() {
                    return Err("Enter a reason for changing resource availability".into());
                }
                *target = reason.into();
            }
            _ => {}
        }
        return Ok(command);
    }
    let project_path = app
        .tree
        .get(dialog.project)
        .and_then(|node| node.project_path())
        .ok_or("The selected project is no longer open")?
        .to_string_lossy()
        .into_owned();
    let link = |resource_id, slug: String| ProjectResourceLink {
        project_id: ResourceProjectId(String::new()),
        resource_id,
        slug,
        project_path: project_path.clone(),
        desired_instruction_revision: 0,
        created_at_unix_millis: 0,
        sync_debt: None,
    };
    if let DialogKind::Link { selected, query } = &dialog.kind {
        let available = candidates(app, query);
        let definition = match &dialog.selected_resource {
            Some(id) => available.iter().find(|definition| &definition.id == id),
            None => available.get(*selected),
        }
        .cloned()
        .ok_or("Choose an existing resource")?;
        let local_slug = if dialog.fields[1].is_empty() {
            slug(&definition.title)
        } else {
            dialog.fields[1].clone()
        };
        return Ok(ResourceCommand::Link {
            link: link(definition.id, local_slug),
        });
    }
    if physical_collision(app, dialog).is_some() {
        return Err(
            "This hardware already has a resource. Choose Link this resource (Ctrl+L).".into(),
        );
    }
    if dialog.fields[0].trim().is_empty() {
        return Err("Enter a resource title".into());
    }
    let id = match &dialog.kind {
        DialogKind::Edit(id) => id.clone(),
        _ => dialog.create_id.clone(),
    };
    let definition = ResourceDefinition {
        id: id.clone(),
        title: dialog.fields[0].clone(),
        policy: ResourcePolicy::ExclusiveFifo,
        created_at_unix_millis: app
            .resources
            .record(&id)
            .map_or(0, |record| record.definition.created_at_unix_millis),
        updated_at_unix_millis: app
            .resources
            .record(&id)
            .map_or(0, |record| record.definition.updated_at_unix_millis),
        description: dialog.fields[2].clone(),
        rules: dialog.fields[3].clone(),
        kind: dialog.fields[4].clone(),
        physical_identity: (!dialog.fields[5].trim().is_empty())
            .then(|| dialog.fields[5].trim().to_owned()),
        definition_revision: app
            .resources
            .record(&id)
            .map_or(0, |record| record.definition.definition_revision),
        archived: false,
    };
    if matches!(dialog.kind, DialogKind::Edit(_)) {
        Ok(ResourceCommand::Edit { definition })
    } else {
        let local_slug = if dialog.fields[1].is_empty() {
            slug(&definition.title)
        } else {
            dialog.fields[1].clone()
        };
        Ok(ResourceCommand::Create {
            definition,
            link: link(id, local_slug),
        })
    }
}
fn area(screen: Rect) -> Rect {
    let width = screen.width.min(96);
    let height = screen.height.min(25);
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}
fn safe(text: &str) -> String {
    super::terminal_text(text)
}
fn fit_line(text: &str, width: u16) -> String {
    let mut left = usize::from(width);
    text.chars()
        .take_while(|character| {
            let cells = character.width().unwrap_or(0);
            if cells > left {
                return false;
            }
            left -= cells;
            true
        })
        .collect()
}
fn content(
    app: &App,
    dialog: &ResourceDialog,
    width: u16,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let mut lines = vec![Line::from("Scheduling: Exclusive FIFO (read only)")];
    if let DialogKind::Link { selected, query } = &dialog.kind {
        lines.push(Line::from(fit_line(
            "↑/↓ selects an existing queue; Tab changes input.",
            width,
        )));
        for (index, definition) in candidates(app, query)
            .iter()
            .enumerate()
            .skip(selected.saturating_sub(2))
            .take(5)
        {
            let state = app
                .resources
                .record(&definition.id)
                .map(|record| {
                    super::tree::configured_state_label(
                        record,
                        &app.ui_settings.icons,
                        app.ui_settings.use_stable_glyphs,
                    )
                })
                .unwrap_or("Unknown".into());
            lines.push(Line::from(fit_line(
                &format!(
                    "{} {} · {} · {} · {}",
                    if index == *selected { "▸" } else { " " },
                    safe(&definition.title),
                    safe(&definition.kind),
                    state,
                    definition.id
                ),
                width,
            )));
        }
    }
    if let DialogKind::Link { selected, query } = &dialog.kind {
        if let Some(definition) = candidates(app, query).get(*selected) {
            let origins = app
                .resources
                .catalog
                .as_ref()
                .map(|catalog| {
                    catalog
                        .links
                        .iter()
                        .filter(|link| link.resource_id == definition.id)
                        .map(|link| safe(&link.project_path))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            lines.push(Line::from(format!("Selected origin: {origins}")));
        }
    }
    if let DialogKind::Link { selected, query } = &dialog.kind {
        if let Some(definition) = candidates(app, query).get(*selected) {
            if let Some(record) = app.resources.record(&definition.id) {
                let owner = record
                    .reservations
                    .iter()
                    .find(|claim| claim.state.occupied())
                    .map(|claim| safe(&claim.agent.label))
                    .unwrap_or("none".into());
                let count = record
                    .reservations
                    .iter()
                    .filter(|claim| matches!(claim.state, ReservationState::Queued))
                    .count();
                lines.push(Line::from(format!("Owner: {owner} · waiting: {count}")));
            }
        }
    }
    if let DialogKind::Confirm { command, .. } = &dialog.kind {
        let reservation_id = match command.as_ref() {
            ResourceCommand::Revoke { reservation_id, .. }
            | ResourceCommand::CancelByUser { reservation_id, .. } => Some(reservation_id),
            _ => None,
        };
        let resource_id = match command.as_ref() {
            ResourceCommand::ConfirmClear { resource_id, .. }
            | ResourceCommand::SetGate { resource_id, .. }
            | ResourceCommand::Archive { resource_id }
            | ResourceCommand::Unlink { resource_id, .. } => Some(resource_id),
            _ => None,
        };
        let record = app.resources.catalog.as_ref().and_then(|catalog| {
            catalog.resources.values().find(|record| {
                resource_id == Some(&record.definition.id)
                    || reservation_id.is_some_and(|id| {
                        record
                            .reservations
                            .iter()
                            .any(|reservation| &reservation.id == id)
                    })
            })
        });
        if let Some(record) = record {
            let owner = record
                .reservations
                .iter()
                .find(|reservation| reservation.state.occupied())
                .map(|reservation| {
                    format!("{} ({})", safe(&reservation.agent.label), reservation.id)
                })
                .unwrap_or("none".into());
            lines.push(Line::from(format!(
                "Resource {} · current owner {owner} · captured revision {}",
                safe(&record.definition.title),
                dialog
                    .expected_revision
                    .map_or("unknown".into(), |revision| revision.to_string())
            )));
        }
        lines.push(Line::from(format!("Action: {command:?}")));
        lines.push(Line::from(
            "Confirm hardware is safe before clearance. This does not stop a workload.",
        ));
    }
    let mut ranges = Vec::new();
    for (index, label) in dialog.labels().iter().enumerate() {
        let start = Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(width.max(1));
        let style = if index == dialog.field {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let label = format!("{} {label}", if index == dialog.field { "▸" } else { " " });
        let mut raw = dialog.fields[index].clone();
        if index == dialog.field {
            let cursor = dialog.cursor.min(raw.len());
            raw.insert_str(cursor, "▏");
        }
        let value = safe(&raw);
        let gap = usize::from(width)
            .saturating_sub(label.width() + value.lines().next().unwrap_or("").width());
        lines.push(Line::from(vec![
            Span::styled(label, style),
            crate::value_control::leader_span(gap),
            Span::styled(
                if value.is_empty() {
                    " [empty]".into()
                } else {
                    value
                },
                style,
            ),
        ]));
        let end = Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(width.max(1));
        ranges.push((start, end.max(start + 1)));
    }
    (lines, ranges)
}
fn scroll_for(dialog: &ResourceDialog, ranges: &[(usize, usize)], height: u16) -> usize {
    ranges.get(dialog.field).map_or(0, |(start, _)| {
        start.saturating_sub(usize::from(height.saturating_sub(2)))
    })
}
pub fn render(app: &App, frame: &mut Frame) {
    let Some(dialog) = &app.resources.dialog else {
        return;
    };
    let area = area(frame.area());
    let title = match dialog.kind {
        DialogKind::Estimate { .. } => "Update own reservation estimate",
        DialogKind::Create => "New shared resource",
        DialogKind::Link { .. } => "Link existing resource",
        DialogKind::Edit(_) => "Edit shared resource — global",
        DialogKind::Confirm { .. } => "Confirm resource management action",
    };
    frame.render_widget(Clear, area);
    let block = crate::theme::block(true).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let (lines, ranges) = content(app, dialog, body.width);
    let scroll = scroll_for(dialog, &ranges, body.height);
    frame.render_widget(
        Paragraph::new(super::sanitized_lines(lines))
            .wrap(Wrap { trim: false })
            .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0)),
        body,
    );
    let status = if let Some(error) = &app.resources.error {
        error.as_str()
    } else if app.resources.pending.is_some() {
        "Saving / loading authoritative resource state…"
    } else {
        "F1 help · Tab field · Enter submit · Ctrl+R retry exact request"
    };
    if inner.height >= 3 {
        frame.render_widget(
            Paragraph::new(safe(status)).style(Style::default().fg(
                if app.resources.error.is_some() {
                    Color::Red
                } else {
                    Color::DarkGray
                },
            )),
            Rect::new(inner.x, inner.bottom() - 3, inner.width, 1),
        );
    }
    if inner.height >= 2 {
        frame.render_widget(
            Paragraph::new(if physical_collision(app, dialog).is_some() {
                "[Submit]  [Cancel]  [Link this resource: Ctrl+L]"
            } else {
                "[Submit]  [Cancel] · Esc cancels"
            })
            .style(Style::default().fg(Color::Cyan)),
            Rect::new(inner.x, inner.bottom() - 2, inner.width, 1),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slug_handles_unicode_and_punctuation() {
        assert_eq!(slug("GPU / test board!"), "gpu-test-board");
        assert_eq!(slug("é GPU"), "gpu");
    }
    #[test]
    fn create_and_link_capture_project() {
        assert_eq!(ResourceDialog::create(NodeId(8)).project, NodeId(8));
        assert_eq!(ResourceDialog::link(NodeId(9)).project, NodeId(9));
    }
}

pub(super) fn autosave(app: &mut App, now: std::time::Instant) -> bool {
    let Some(mut dialog) = app.resources.dialog.take() else {
        return false;
    };
    let due = matches!(dialog.kind, DialogKind::Edit(_))
        && !dialog.submitted
        && app.resources.pending.is_none()
        && dialog.dirty_at.is_some_and(|edited| {
            now.saturating_duration_since(edited) >= std::time::Duration::from_millis(600)
        });
    if due {
        dialog.dirty_at = None;
        match command(app, &dialog) {
            Ok(command) => {
                app.resource_request_full(
                    dialog.project,
                    ResourceOperation::Mutate(command),
                    None,
                    dialog.expected_revision,
                    Some(dialog.request_key.clone()),
                    dialog.actor.as_ref().map(|actor| actor.pane_id),
                );
                dialog.submitted = app.resources.pending.is_some();
            }
            Err(message) => app.resources.error = Some(message),
        }
    }
    app.resources.dialog = Some(dialog);
    due
}

#[cfg(test)]
mod autosave_tests {
    use super::*;
    #[test]
    fn metadata_autosave_waits_for_debounce_and_keeps_revision_fence() {
        let (tree, catalog, project, _) = super::super::tree::tests::fixture();
        let definition = catalog
            .resources
            .values()
            .next()
            .unwrap()
            .definition
            .clone();
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        app.tree = tree;
        app.resources.catalog = Some(catalog);
        let start = std::time::Instant::now();
        let mut dialog = ResourceDialog::edit(project, &definition);
        dialog.expected_revision = Some(4);
        dialog.dirty_at = Some(start);
        app.resources.dialog = Some(dialog);
        assert!(!autosave(
            &mut app,
            start + std::time::Duration::from_millis(599)
        ));
        assert!(autosave(
            &mut app,
            start + std::time::Duration::from_millis(600)
        ));
        let dialog = app.resources.dialog.as_ref().unwrap();
        assert_eq!(dialog.expected_revision, Some(4));
        assert!(dialog.dirty_at.is_none());
    }
}
#[cfg(test)]
mod recovery_and_link_tests {
    use super::*;
    #[test]
    fn recovery_requires_authored_reason_and_preserves_action() {
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        let mut dialog = ResourceDialog::confirm(
            NodeId(7),
            ResourceCommand::ConfirmClear {
                resource_id: ResourceId::from("gpu"),
                reason: "placeholder".into(),
            },
            Some("token".into()),
        );
        dialog.fields[0] = "CONFIRM".into();
        assert!(command(&app, &dialog).unwrap_err().contains("reason"));
        dialog.fields[1] = "Inspected fixture; no work remains".into();
        assert!(
            matches!(command(&app,&dialog).unwrap(),ResourceCommand::ConfirmClear{reason,..} if reason==dialog.fields[1])
        );
        app.resources.dialog = Some(dialog);
        assert!(handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        ));
        assert!(app.resources.dialog.is_none());
    }
    #[test]
    fn physical_collision_offers_explicit_link_and_searches_kind_and_origin() {
        let (tree, mut catalog, project, _) = super::super::tree::tests::fixture();
        catalog
            .resources
            .values_mut()
            .next()
            .unwrap()
            .definition
            .physical_identity = Some("usb:fixture-board".into());
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        app.tree = tree;
        app.resources.catalog = Some(catalog);
        assert_eq!(candidates(&app, "gpu").len(), 1);
        assert_eq!(candidates(&app, "/synthetic-resource-project").len(), 1);
        let mut dialog = ResourceDialog::create(project);
        dialog.fields[0] = "Same board".into();
        dialog.fields[5] = "USB:fixture-board".into();
        assert!(command(&app, &dialog)
            .unwrap_err()
            .contains("Link this resource"));
        app.resources.dialog = Some(dialog);
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        );
        let dialog = app.resources.dialog.as_ref().unwrap();
        assert!(matches!(dialog.kind, DialogKind::Link { .. }));
        assert_eq!(dialog.selected_resource, Some(ResourceId::from("gpu")));
    }
}
#[cfg(test)]
mod editing_tests {
    use super::*;
    #[test]
    fn unicode_cursor_editing_and_multiline_paste_preserve_utf8() {
        let mut app = App::new(
            "test".into(),
            std::path::PathBuf::from("/synthetic-resource-project"),
        );
        app.resources.dialog = Some(ResourceDialog::create(NodeId(7)));
        handle_event(&mut app, &Event::Paste("GPU αβ".into()));
        for code in [
            KeyCode::Left,
            KeyCode::Backspace,
            KeyCode::Char('γ'),
            KeyCode::Home,
            KeyCode::Delete,
        ] {
            handle_event(
                &mut app,
                &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            );
        }
        assert_eq!(app.resources.dialog.as_ref().unwrap().fields[0], "PU γβ");
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        );
        handle_event(&mut app, &Event::Paste("line two".into()));
        assert!(app.resources.dialog.as_ref().unwrap().fields[0].ends_with("β\nline two"));
    }
}
