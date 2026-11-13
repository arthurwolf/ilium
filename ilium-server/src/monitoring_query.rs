//! Deterministic paging and compact output for monitoring events.

use ilium_core::NodeId;
use ilium_ipc::{MonitoringEvent, MonitoringEventKind, MonitoringFormat, MonitoringQuery};

fn validate_consumer(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err("invalid consumer name".into());
    }
    Ok(())
}

pub(crate) fn acknowledge(
    events: &crate::state::ServerEvents,
    consumer: &str,
    cursor: &str,
) -> Result<u64, String> {
    validate_consumer(consumer)?;
    let sequence = {
        let log = events
            .monitoring
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let sequence = parse_cursor(cursor, &log.epoch().to_string()).map_err(str::to_owned)?;
        log.read_after(sequence)
            .map_err(|error| format!("invalid event cursor: {error:?}"))?;
        sequence
    };
    let mut consumers = events
        .monitoring_consumers
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if consumers.len() >= 256 && !consumers.contains_key(consumer) {
        return Err("monitoring consumer capacity reached".into());
    }
    events
        .monitoring_digest_consumers
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .acknowledge(
            consumer,
            sequence,
            delivery_id(cursor).map_err(str::to_owned)?,
        )
        .map_err(str::to_owned)?;
    let acknowledged = consumers.entry(consumer.to_owned()).or_insert(0);
    *acknowledged = (*acknowledged).max(sequence);
    Ok(*acknowledged)
}

#[cfg(test)]
pub(crate) async fn read_query(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
) -> Result<String, String> {
    read_query_inner(events, query, None).await
}

pub(crate) async fn read_query_with_state(
    state: &std::sync::Arc<crate::state::ServerState>,
    query: &MonitoringQuery,
) -> Result<String, String> {
    if query.view == ilium_ipc::MonitoringView::Brief {
        let pane = query
            .pane_id
            .ok_or_else(|| "pane brief requires a pane ID".to_owned())?;
        if query.follow {
            return Err("pane brief does not support follow".into());
        }
        return read_brief(state, pane).await;
    }
    read_query_inner(&state.events, query, Some(state)).await
}

async fn read_query_inner(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
    state: Option<&std::sync::Arc<crate::state::ServerState>>,
) -> Result<String, String> {
    if let Some(name) = &query.consumer {
        validate_consumer(name)?;
    }
    let mut effective = query.clone();
    if effective.since.is_none() {
        let consumer = effective.consumer.as_deref().unwrap_or("default");
        let cursor = events
            .monitoring_consumers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(consumer)
            .copied()
            .unwrap_or(0);
        let epoch = events
            .monitoring
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .epoch();
        effective.since = Some(format!("{epoch}:{cursor}"));
    }
    let mut changes = events.subscribe_monitoring();
    loop {
        if let Some(state) = state {
            crate::monitoring_signals::refresh(state).await;
        }
        let replies = if effective.view == ilium_ipc::MonitoringView::Digest {
            match state {
                Some(state) => crate::monitoring_transcript::final_replies(state, &effective).await,
                None => std::collections::HashMap::new(),
            }
        } else {
            std::collections::HashMap::new()
        };
        let (text, cursor, omitted) = render_query_with_replies(events, &effective, &replies)?;
        let quiet = match effective.format {
            MonitoringFormat::Compact => text.starts_with("quiet "),
            MonitoringFormat::Json => {
                text.contains("\"events\":[]") || text.contains("\"quiet\":true")
            }
        };
        if !effective.follow || !quiet || omitted > 0 {
            return Ok(text);
        }
        // Filtered records can be skipped by this wait without acknowledging
        // them for the consumer; this also prevents a noisy stream evicting
        // the waiter's original cursor before the next relevant change.
        if let Some((epoch, _)) = effective
            .since
            .as_deref()
            .and_then(|value| value.rsplit_once(':'))
        {
            effective.since = Some(format!("{epoch}:{cursor}"));
        }
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            crate::state::wait_for_monitoring_change(
                &mut changes,
                crate::state::MONITORING_COALESCE_WINDOW,
            ),
        )
        .await
        {
            Ok(Ok(_)) | Err(_) => {}
            Ok(Err(_)) => return Err("event stream closed".into()),
        }
    }
}

/// Render one bounded read under the event log lock; no terminal data is read.
#[cfg(test)]
pub(crate) fn render_query(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
) -> Result<(String, u64, usize), String> {
    render_query_with_replies(events, query, &std::collections::HashMap::new())
}

fn render_query_with_replies(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
    replies: &std::collections::HashMap<(NodeId, u64), String>,
) -> Result<(String, u64, usize), String> {
    if query.max_items == 0
        || query.max_items > 4096
        || query.max_chars == 0
        || query.max_chars > 1_000_000
    {
        return Err("invalid output budget".into());
    }
    let minimum_severity = match query.min_severity.as_deref() {
        None if !query.kinds.is_empty() => Severity::Info,
        None | Some("notice") => Severity::Notice,
        Some("info") => Severity::Info,
        Some("warning") => Severity::Warning,
        Some("error") => Severity::Error,
        Some(_) => return Err("invalid severity".into()),
    };
    let filter = EventFilter {
        kinds: query.kinds.clone(),
        minimum_severity,
        pane_id: query.pane_id,
        project: query.project.clone(),
        agent: query.agent.clone(),
    };
    let log = events
        .monitoring
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let epoch = log.epoch().to_string();
    let after = query
        .since
        .as_deref()
        .map(|cursor| parse_cursor(cursor, &epoch))
        .transpose()
        .map_err(|error| {
            format!(
                "{error}; resume_from={epoch}:{}; latest={epoch}:{}",
                log.resume_cursor(),
                log.latest_cursor()
            )
        })?
        .unwrap_or_else(|| {
            events
                .monitoring_consumers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(query.consumer.as_deref().unwrap_or("default"))
                .copied()
                .unwrap_or(0)
        });
    let mut recorded = log.read_after(after).map_err(|error| {
        format!(
            "invalid event cursor: {error:?}; resume_from={epoch}:{}; latest={epoch}:{}",
            log.resume_cursor(),
            log.latest_cursor()
        )
    })?;
    if query.view == ilium_ipc::MonitoringView::Digest {
        for event in &mut recorded {
            if event.kind == MonitoringEventKind::Done {
                if let Some(reply) = event
                    .pane_id
                    .and_then(|pane| replies.get(&(pane, event.cursor)))
                {
                    event.summary = reply.clone();
                } else if event.summary == "Agent finished" {
                    event.summary = "Final reply unavailable".into();
                }
            }
        }
        return render_digest_query(
            events,
            query,
            &recorded,
            &filter,
            &epoch,
            after,
            log.working_count(),
        );
    }
    let known_contexts = events
        .monitoring_digest_consumers
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contexts(query.consumer.as_deref().unwrap_or("default"));
    match query.format {
        MonitoringFormat::Compact => {
            let page = compact_page_with_context(
                &recorded,
                after,
                &filter,
                &epoch,
                query.max_items,
                query.max_chars.saturating_sub(22),
                &known_contexts,
            )
            .map_err(str::to_owned)?;
            let contexts = recorded
                .iter()
                .filter(|event| {
                    event.cursor > after && event.cursor <= page.cursor && filter.matches(event)
                })
                .filter_map(|event| {
                    event
                        .pane_id
                        .map(|pane| (pane, ilium_control::digest::context_fingerprint(event)))
                })
                .collect::<Vec<_>>();
            let text = receipt_text(
                events,
                query,
                &epoch,
                page.text,
                page.cursor,
                &[],
                &contexts,
            )?;
            Ok((text, page.cursor, page.omitted))
        }
        MonitoringFormat::Json => {
            let page = select_page(&recorded, after, &filter, query.max_items);
            let cursors = page_cursors(&recorded, after, &filter, page.events.len());
            debug_assert_eq!(cursors.last().copied(), Some(page.cursor));
            let lines = page
                .events
                .iter()
                .map(|event| {
                    let mut displayed = (*event).clone();
                    if event.pane_id.is_some_and(|pane| {
                        known_contexts.get(&pane)
                            == Some(&ilium_control::digest::context_fingerprint(event))
                    }) {
                        displayed.project = None;
                        displayed.title = None;
                        displayed.agent = None;
                    }
                    serde_json::to_string(&displayed).map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut prefix_chars = vec![0usize];
            for line in &lines {
                let comma = usize::from(prefix_chars.len() > 1);
                prefix_chars
                    .push(prefix_chars.last().copied().unwrap_or(0) + comma + line.chars().count());
            }
            for count in (0..=page.events.len()).rev() {
                let omitted = page.omitted + page.events.len() - count;
                let cursor = cursors[count];
                let encoded_cursor = serde_json::to_string(&format!("{epoch}:{cursor}"))
                    .map_err(|error| error.to_string())?;
                let suffix = format!("],\"cursor\":{encoded_cursor},\"more\":{omitted}}}\n");
                let prefix = "{\"events\":[";
                if prefix.chars().count() + prefix_chars[count] + suffix.chars().count()
                    <= query.max_chars.saturating_sub(22)
                {
                    let text = format!("{prefix}{}{suffix}", lines[..count].join(","));
                    let contexts = page.events[..count]
                        .iter()
                        .filter_map(|event| {
                            event.pane_id.map(|pane| {
                                (pane, ilium_control::digest::context_fingerprint(event))
                            })
                        })
                        .collect::<Vec<_>>();
                    let text = receipt_text(events, query, &epoch, text, cursor, &[], &contexts)?;
                    return Ok((text, cursor, omitted));
                }
            }
            Err("output budget is too small to report a cursor and overflow".into())
        }
    }
}

fn receipt_text(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
    epoch: &str,
    text: String,
    cursor: u64,
    delivered: &[(NodeId, u64)],
    contexts: &[(NodeId, u64)],
) -> Result<String, String> {
    let receipt = events
        .monitoring_digest_consumers
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .stage(
            query.consumer.as_deref().unwrap_or("default"),
            cursor,
            delivered,
            contexts,
        )
        .map_err(str::to_owned)?;
    decorate_cursor(text, query.format, epoch, cursor, receipt)
}

fn decorate_cursor(
    mut text: String,
    format: MonitoringFormat,
    epoch: &str,
    cursor: u64,
    receipt: u64,
) -> Result<String, String> {
    let base = format!("{epoch}:{cursor}");
    let encoded = format!("{base}@{receipt}");
    match format {
        MonitoringFormat::Json => {
            let mut value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            value["cursor"] = serde_json::Value::String(encoded);
            Ok(value.to_string() + "\n")
        }
        MonitoringFormat::Compact => {
            let start = text.rfind(&base).ok_or("missing rendered cursor")?;
            text.replace_range(start..start + base.len(), &encoded);
            Ok(text)
        }
    }
}

/// Coordinate rendering and pending delivery under one consumer lock. No I/O.
fn render_digest_query(
    events: &crate::state::ServerEvents,
    query: &MonitoringQuery,
    recorded: &[MonitoringEvent],
    filter: &EventFilter,
    epoch: &str,
    after: u64,
    working: usize,
) -> Result<(String, u64, usize), String> {
    use ilium_control::digest::{render_digest, DigestOptions};
    let consumer = query.consumer.as_deref().unwrap_or("default");
    let mut consumers = events
        .monitoring_digest_consumers
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let acknowledged = consumers.evidence(consumer);
    let contexts = consumers.contexts(consumer);
    let selected: Vec<_> = recorded
        .iter()
        .map(|event| {
            let mut selected = event.clone();
            if !filter.matches(event)
                && !matches!(
                    event.kind,
                    MonitoringEventKind::Started
                        | MonitoringEventKind::Idle
                        | MonitoringEventKind::Done
                        | MonitoringEventKind::Closed
                )
            {
                selected.kind = MonitoringEventKind::Progress;
            }
            if event.kind == MonitoringEventKind::Done && !filter.matches(event) {
                selected.kind = MonitoringEventKind::Idle;
            }
            selected
        })
        .collect();
    let mut budget = query.max_chars.saturating_sub(22);
    loop {
        let page = render_digest(
            &selected,
            &DigestOptions {
                after,
                epoch,
                working,
                max_items: query.max_items,
                max_chars: budget,
                acknowledged: &acknowledged,
                contexts: &contexts,
            },
        )
        .map_err(str::to_owned)?;
        let cursor = page.cursor;
        let text = match query.format {
            MonitoringFormat::Compact => page.text,
            MonitoringFormat::Json => {
                let mut lines: Vec<_> = page.text.lines().collect();
                lines.pop(); // Compact footer is represented by typed JSON fields.
                serde_json::json!({"digest": lines, "cursor": format!("{epoch}:{cursor}"),
                    "more": page.omitted, "working": working, "quiet": page.delivered.is_empty() && page.omitted == 0}).to_string() + "\n"
            }
        };
        let length = text.chars().count();
        if length <= query.max_chars.saturating_sub(22) {
            let receipt = consumers
                .stage(consumer, cursor, &page.delivered, &page.contexts)
                .map_err(str::to_owned)?;
            let text = decorate_cursor(text, query.format, epoch, cursor, receipt)?;
            return Ok((text, cursor, page.omitted));
        }
        budget = budget
            .checked_sub(length - query.max_chars.saturating_sub(22))
            .filter(|budget| *budget > 0)
            .ok_or_else(|| {
                "output budget is too small to report a cursor and overflow".to_owned()
            })?;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Severity {
    Info,
    Notice,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct EventFilter {
    pub(crate) kinds: Vec<MonitoringEventKind>,
    pub(crate) minimum_severity: Severity,
    pub(crate) pane_id: Option<NodeId>,
    pub(crate) project: Option<String>,
    pub(crate) agent: Option<String>,
}

impl Default for EventFilter {
    fn default() -> Self {
        Self {
            kinds: Vec::new(),
            minimum_severity: Severity::Notice,
            pane_id: None,
            project: None,
            agent: None,
        }
    }
}

fn severity(kind: MonitoringEventKind) -> Severity {
    match kind {
        MonitoringEventKind::Progress
        | MonitoringEventKind::Started
        | MonitoringEventKind::Idle
        | MonitoringEventKind::ResourceChanged => Severity::Info,
        MonitoringEventKind::Done | MonitoringEventKind::Closed => Severity::Notice,
        MonitoringEventKind::NeedsInput => Severity::Warning,
        MonitoringEventKind::Error | MonitoringEventKind::Stuck => Severity::Error,
    }
}

impl EventFilter {
    fn matches(&self, event: &MonitoringEvent) -> bool {
        severity(event.kind) >= self.minimum_severity
            && (self.kinds.is_empty() || self.kinds.contains(&event.kind))
            && self.pane_id.is_none_or(|pane| event.pane_id == Some(pane))
            && self.project.as_ref().is_none_or(|wanted| {
                event
                    .project
                    .as_ref()
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(wanted))
            })
            && self.agent.as_ref().is_none_or(|wanted| {
                event
                    .agent
                    .as_ref()
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(wanted))
            })
    }
}

pub(crate) struct EventPage<'a> {
    pub(crate) events: Vec<&'a MonitoringEvent>,
    pub(crate) cursor: u64,
    pub(crate) omitted: usize,
}

pub(crate) fn select_page<'a>(
    events: &'a [MonitoringEvent],
    after: u64,
    filter: &EventFilter,
    max_items: usize,
) -> EventPage<'a> {
    let mut page = EventPage {
        events: Vec::new(),
        cursor: after,
        omitted: 0,
    };
    let mut blocked = false;
    for event in events.iter().filter(|event| event.cursor > after) {
        if filter.matches(event) {
            if page.events.len() < max_items && !blocked {
                page.events.push(event);
                page.cursor = event.cursor;
            } else {
                page.omitted += 1;
                blocked = true;
            }
        } else if !blocked {
            page.cursor = event.cursor;
        }
    }
    page
}

fn page_cursors(
    events: &[MonitoringEvent],
    after: u64,
    filter: &EventFilter,
    included_limit: usize,
) -> Vec<u64> {
    let mut cursors = vec![after];
    let mut included = 0;
    for event in events.iter().filter(|event| event.cursor > after) {
        if filter.matches(event) {
            if included == included_limit {
                break;
            }
            included += 1;
            cursors.push(event.cursor);
        } else {
            cursors[included] = event.cursor;
        }
    }
    cursors
}

pub(crate) struct CompactPage {
    pub(crate) text: String,
    pub(crate) cursor: u64,
    pub(crate) omitted: usize,
}

pub(crate) fn parse_cursor(cursor: &str, epoch: &str) -> Result<u64, &'static str> {
    let cursor = cursor.split_once('@').map_or(cursor, |(base, _)| base);
    let (actual_epoch, sequence) = cursor.rsplit_once(':').ok_or("invalid cursor")?;
    if actual_epoch != epoch {
        return Err("cursor belongs to a different server lifetime");
    }
    sequence.parse().map_err(|_| "invalid cursor sequence")
}

fn delivery_id(cursor: &str) -> Result<Option<u64>, &'static str> {
    cursor
        .split_once('@')
        .map(|(_, id)| id.parse().map_err(|_| "invalid delivery receipt"))
        .transpose()
}

fn kind_name(kind: MonitoringEventKind) -> &'static str {
    kind.as_str()
}

pub(crate) fn parse_kind(name: &str) -> Option<MonitoringEventKind> {
    MonitoringEventKind::parse(name)
}

#[cfg(test)]
pub(crate) fn compact_page(
    events: &[MonitoringEvent],
    after: u64,
    filter: &EventFilter,
    epoch: &str,
    max_items: usize,
    max_chars: usize,
) -> Result<CompactPage, &'static str> {
    compact_page_with_context(
        events,
        after,
        filter,
        epoch,
        max_items,
        max_chars,
        &std::collections::HashMap::new(),
    )
}

fn compact_page_with_context(
    events: &[MonitoringEvent],
    after: u64,
    filter: &EventFilter,
    epoch: &str,
    max_items: usize,
    max_chars: usize,
    known_contexts: &std::collections::HashMap<NodeId, u64>,
) -> Result<CompactPage, &'static str> {
    let page = select_page(events, after, filter, max_items);
    let cursors = page_cursors(events, after, filter, page.events.len());
    debug_assert_eq!(cursors.last().copied(), Some(page.cursor));
    let mut lines = Vec::with_capacity(page.events.len());
    let mut prefix_chars = vec![0usize];
    let mut contexts = std::collections::HashMap::new();
    for event in &page.events {
        let severity = match severity(event.kind) {
            Severity::Info => "I",
            Severity::Notice => "N",
            Severity::Warning => "W",
            Severity::Error => "E",
        };
        let pane = event
            .pane_id
            .map(|id| id.0.to_string())
            .unwrap_or_else(|| "-".into());
        let context = match (&event.project, &event.title) {
            (Some(project), Some(title)) => format!("{project}/{title}"),
            (Some(project), None) => project.clone(),
            (None, Some(title)) => title.clone(),
            (None, None) => "-".into(),
        };
        let context = match event.pane_id {
            Some(id)
                if contexts.get(&id) == Some(&context)
                    || known_contexts.get(&id)
                        == Some(&ilium_control::digest::context_fingerprint(event)) =>
            {
                "-".to_owned()
            }
            Some(id) => {
                contexts.insert(id, context.clone());
                context
            }
            None => context,
        };
        let drill = event
            .pane_id
            .map(|id| format!(" -> ilium ctl pane {} --brief", id.0))
            .unwrap_or_default();
        let line = format!(
            "{severity} {pane} {context} {} {}{drill}\n",
            kind_name(event.kind),
            event.summary
        );
        prefix_chars.push(prefix_chars.last().copied().unwrap_or(0) + line.chars().count());
        lines.push(line);
    }
    for count in (0..=page.events.len()).rev() {
        let omitted = page.omitted + page.events.len() - count;
        let cursor = cursors[count];
        let suffix = if omitted > 0 {
            format!("+{omitted} more (cursor {epoch}:{cursor})\n")
        } else if count == 0 {
            format!("quiet cursor={epoch}:{cursor}\n")
        } else {
            format!("cursor={epoch}:{cursor}\n")
        };
        if prefix_chars[count] + suffix.chars().count() <= max_chars {
            let mut text = lines[..count].concat();
            text.push_str(&suffix);
            return Ok(CompactPage {
                text,
                cursor,
                omitted,
            });
        }
    }
    Err("output budget is too small to report a cursor and overflow")
}

/// Minimal pane read shared by the command and HTTP adapters.
pub(crate) async fn read_brief(
    state: &std::sync::Arc<crate::state::ServerState>,
    pane: NodeId,
) -> Result<String, String> {
    let pane_state = |tree: &ilium_core::Tree| -> Result<&'static str, String> {
        match &tree
            .get(pane)
            .ok_or_else(|| "pane not found".to_owned())?
            .kind
        {
            ilium_core::NodeKind::Pane { status, .. } => Ok(ilium_control::pane_state_name(status)),
            _ => Err("node is not a pane".to_owned()),
        }
    };
    let status = {
        let tree = state.tree.read().await;
        pane_state(&tree)?
    };
    let message = crate::monitoring_transcript::read_reply(state, pane, true).await;
    let current = {
        let tree = state.tree.read().await;
        pane_state(&tree)?
    };
    let message = if current == status { message } else { None };
    let mut brief = ilium_control::brief::pane_brief(pane, current, message.as_deref(), None, 0);
    brief.state_age_seconds = state
        .events
        .monitoring
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state_age_seconds(pane, current);
    serde_json::to_string(&brief)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;
    use ilium_ipc::{MonitoringEvent, MonitoringEventKind};

    fn review_query(view: ilium_ipc::MonitoringView) -> MonitoringQuery {
        MonitoringQuery {
            view,
            since: None,
            consumer: Some("review-manager".into()),
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 4000,
            format: MonitoringFormat::Json,
        }
    }

    #[test]
    fn review_ack_commits_only_the_page_that_was_actually_delivered() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(Some(NodeId(1)), MonitoringEventKind::Done, "old completion");
        events.record_monitoring(
            Some(NodeId(2)),
            MonitoringEventKind::NeedsInput,
            "first question?",
        );
        events.record_monitoring(
            Some(NodeId(3)),
            MonitoringEventKind::NeedsInput,
            "unseen question?",
        );
        let mut query = review_query(ilium_ipc::MonitoringView::Digest);
        query.max_items = 2;
        let _omitted_by_aggregate = render_query(&events, &query).unwrap();
        query.max_items = 1;
        let (shown, _, _) = render_query(&events, &query).unwrap();
        let shown: serde_json::Value = serde_json::from_str(&shown).unwrap();
        acknowledge(&events, "review-manager", shown["cursor"].as_str().unwrap()).unwrap();
        query.max_items = 12;
        let (remaining, _, _) = render_query(&events, &query).unwrap();
        assert!(remaining.contains("unseen question?"), "{remaining}");
        assert!(!remaining.contains("first question?"), "{remaining}");
    }

    #[test]
    fn review_filtered_completion_still_resolves_an_old_question() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(
            Some(NodeId(2)),
            MonitoringEventKind::NeedsInput,
            "resolved question?",
        );
        events.record_monitoring(Some(NodeId(2)), MonitoringEventKind::Done, "completion");
        let mut query = review_query(ilium_ipc::MonitoringView::Digest);
        query.min_severity = Some("warning".into());
        let (text, _, _) = render_query(&events, &query).unwrap();
        assert!(!text.contains("resolved question?"), "{text}");
    }

    #[tokio::test]
    async fn review_expired_or_foreign_cursors_offer_an_actionable_resume_cursor() {
        let events = crate::state::ServerEvents::new();
        for _ in 0..4100 {
            events.record_monitoring(Some(NodeId(2)), MonitoringEventKind::Error, "fixture");
        }
        let epoch = events.monitoring.lock().unwrap().epoch();
        let mut query = review_query(ilium_ipc::MonitoringView::Events);
        for since in [None, Some("foreign:1".into())] {
            query.since = since;
            let error = read_query(&events, &query).await.unwrap_err();
            assert!(error.contains(&format!("resume_from={epoch}:")), "{error}");
        }
    }

    #[test]
    fn review_acknowledged_static_context_is_not_repeated_across_pages() {
        for view in [
            ilium_ipc::MonitoringView::Events,
            ilium_ipc::MonitoringView::Digest,
        ] {
            let events = crate::state::ServerEvents::new();
            let mut tree = ilium_core::Tree::new();
            let project = tree.add_project("/fixture/static-context".into()).unwrap();
            let pane = tree
                .add_pane(
                    project,
                    "unique-context-title",
                    ilium_core::PaneContentKind::Terminal,
                )
                .unwrap();
            events
                .monitoring
                .lock()
                .unwrap()
                .replace_tree_context(&tree);
            events.record_monitoring(Some(pane), MonitoringEventKind::Error, "first failure");
            let query = review_query(view);
            let (first, _, _) = render_query(&events, &query).unwrap();
            assert!(first.contains("unique-context-title"));
            let first: serde_json::Value = serde_json::from_str(&first).unwrap();
            acknowledge(&events, "review-manager", first["cursor"].as_str().unwrap()).unwrap();
            events.record_monitoring(Some(pane), MonitoringEventKind::Error, "different failure");
            let (next, _, _) = render_query(&events, &query).unwrap();
            assert!(!next.contains("unique-context-title"), "{next}");
            assert!(next.contains("different failure"));
        }
    }

    #[test]
    fn acknowledgements_are_scoped_monotonic_and_validate_the_session_cursor() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(Some(NodeId(1)), MonitoringEventKind::Error, "one");
        events.record_monitoring(Some(NodeId(2)), MonitoringEventKind::Error, "two");
        let epoch = events.monitoring.lock().unwrap().epoch();
        assert_eq!(
            acknowledge(&events, "manager", &format!("{epoch}:2")).unwrap(),
            2
        );
        assert_eq!(
            acknowledge(&events, "manager", &format!("{epoch}:1")).unwrap(),
            2
        );
        assert_eq!(
            events.monitoring_consumers.lock().unwrap().get("manager"),
            Some(&2)
        );
        assert!(!events
            .monitoring_consumers
            .lock()
            .unwrap()
            .contains_key("other"));
        assert!(acknowledge(&events, "other", &format!("{epoch}:3")).is_err());
        assert!(acknowledge(&events, "other", "wrong-epoch:1").is_err());
        assert!(acknowledge(&events, "bad\nname", &format!("{epoch}:1")).is_err());
        assert!(!events
            .monitoring_consumers
            .lock()
            .unwrap()
            .contains_key("other"));
    }

    #[tokio::test]
    async fn named_consumer_can_read_from_the_beginning_before_its_first_acknowledgement() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(Some(NodeId(1)), MonitoringEventKind::Error, "failure");
        let query = MonitoringQuery {
            view: ilium_ipc::MonitoringView::Events,
            since: None,
            consumer: Some("manager".into()),
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 4000,
            format: MonitoringFormat::Compact,
        };
        assert!(read_query(&events, &query)
            .await
            .unwrap()
            .contains("failure"));
        let mut invalid = query;
        invalid.consumer = Some("bad\nname".into());
        assert!(read_query(&events, &invalid).await.is_err());
    }

    #[tokio::test]
    async fn named_consumer_reads_only_after_its_acknowledged_cursor() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(Some(NodeId(1)), MonitoringEventKind::Error, "old error");
        events.record_monitoring(Some(NodeId(2)), MonitoringEventKind::Error, "new error");
        events
            .monitoring_consumers
            .lock()
            .unwrap()
            .insert("manager".into(), 1);
        let query = MonitoringQuery {
            view: ilium_ipc::MonitoringView::Events,
            since: None,
            consumer: Some("manager".into()),
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 4000,
            format: MonitoringFormat::Compact,
        };
        let output = read_query(&events, &query).await.unwrap();
        assert!(!output.contains("old error"));
        assert!(output.contains("new error"));
        let mut explicit = query;
        let epoch = events.monitoring.lock().unwrap().epoch();
        explicit.since = Some(format!("{epoch}:0"));
        let output = read_query(&events, &explicit).await.unwrap();
        assert!(output.contains("old error"));
        assert!(output.contains("new error"));
    }

    #[tokio::test]
    async fn follow_blocks_until_matching_change_and_coalesces_the_burst() {
        let events = std::sync::Arc::new(crate::state::ServerEvents::new());
        let epoch = events.monitoring.lock().unwrap().epoch();
        let query = MonitoringQuery {
            view: ilium_ipc::MonitoringView::Events,
            since: Some(format!("{epoch}:0")),
            consumer: None,
            follow: true,
            min_severity: None,
            kinds: vec![MonitoringEventKind::Error],
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 4000,
            format: MonitoringFormat::Compact,
        };
        let waiting_events = events.clone();
        let handle = tokio::spawn(async move { read_query(&waiting_events, &query).await });
        tokio::task::yield_now().await;
        assert!(!handle.is_finished());
        events.record_monitoring(
            Some(NodeId(3)),
            MonitoringEventKind::Progress,
            "routine progress",
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !handle.is_finished(),
            "filtered progress must not wake the manager"
        );
        events.record_monitoring(Some(NodeId(1)), MonitoringEventKind::Error, "first failure");
        events.record_monitoring(
            Some(NodeId(2)),
            MonitoringEventKind::Error,
            "second failure",
        );
        let output = tokio::time::timeout(std::time::Duration::from_secs(8), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(output.contains("first failure"));
        assert!(output.contains("second failure"));
        assert!(!output.contains("routine progress"));
    }

    #[tokio::test]
    async fn json_budget_includes_metadata_and_never_acknowledges_omitted_events() {
        let events = crate::state::ServerEvents::new();
        for pane in 1..=4 {
            events.record_monitoring(
                Some(NodeId(pane)),
                MonitoringEventKind::Error,
                &"failure ".repeat(20),
            );
        }
        let query = MonitoringQuery {
            view: ilium_ipc::MonitoringView::Events,
            since: None,
            consumer: None,
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 300,
            format: MonitoringFormat::Json,
        };
        let (text, cursor, omitted) = render_query(&events, &query).unwrap();
        assert!(text.chars().count() <= query.max_chars);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(omitted > 0);
        assert_eq!(value["more"].as_u64(), Some(omitted as u64));
        assert!(cursor < 4);
    }

    #[test]
    fn rejects_invalid_severity_budgets_and_other_server_cursors() {
        let events = crate::state::ServerEvents::new();
        let mut query = MonitoringQuery {
            view: ilium_ipc::MonitoringView::Events,
            since: None,
            consumer: None,
            follow: false,
            min_severity: Some("severe".into()),
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 12,
            max_chars: 4000,
            format: MonitoringFormat::Compact,
        };
        assert!(render_query(&events, &query).is_err());
        query.min_severity = None;
        query.max_items = 0;
        assert!(render_query(&events, &query).is_err());
        query.max_items = 12;
        query.since = Some("different-server:1".into());
        assert!(render_query(&events, &query).is_err());
    }

    fn event(cursor: u64, kind: MonitoringEventKind) -> MonitoringEvent {
        MonitoringEvent {
            cursor,
            generation: 0,
            pane_id: Some(NodeId(cursor)),
            kind,
            project: None,
            title: None,
            agent: None,
            observed_at_unix_millis: cursor,
            summary: format!("event {cursor}"),
        }
    }

    #[test]
    fn page_does_not_advance_past_a_matching_omitted_event() {
        let events = vec![
            event(1, MonitoringEventKind::Progress),
            event(2, MonitoringEventKind::Error),
            event(3, MonitoringEventKind::Error),
        ];
        let filter = EventFilter {
            kinds: vec![MonitoringEventKind::Error],
            ..EventFilter::default()
        };
        let page = select_page(&events, 0, &filter, 1);
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.cursor)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(page.cursor, 2);
        assert_eq!(page.omitted, 1);
    }

    #[test]
    fn paging_skips_noise_between_delivered_and_omitted_matches() {
        let events = vec![
            event(1, MonitoringEventKind::Error),
            event(2, MonitoringEventKind::Progress),
            event(3, MonitoringEventKind::Error),
            event(4, MonitoringEventKind::Progress),
        ];
        let filter = EventFilter {
            kinds: vec![MonitoringEventKind::Error],
            ..EventFilter::default()
        };
        let page = select_page(&events, 0, &filter, 1);
        assert_eq!(page.cursor, 2);
        assert_eq!(page.omitted, 1);
        let next = select_page(&events, page.cursor, &filter, 1);
        assert_eq!(next.events[0].cursor, 3);
        assert_eq!(next.cursor, 4);
    }

    #[test]
    fn quiet_page_advances_across_filtered_events() {
        let events = vec![
            event(1, MonitoringEventKind::Progress),
            event(2, MonitoringEventKind::Idle),
        ];
        let filter = EventFilter {
            kinds: vec![MonitoringEventKind::Error],
            ..EventFilter::default()
        };
        let page = select_page(&events, 0, &filter, 12);
        assert!(page.events.is_empty());
        assert_eq!(page.cursor, 2);
        assert_eq!(page.omitted, 0);
    }

    #[test]
    fn pane_and_severity_filters_are_combined_on_the_server() {
        let events = vec![
            event(1, MonitoringEventKind::Progress),
            event(2, MonitoringEventKind::Error),
            event(3, MonitoringEventKind::NeedsInput),
        ];
        let filter = EventFilter {
            pane_id: Some(NodeId(2)),
            minimum_severity: Severity::Warning,
            ..EventFilter::default()
        };
        let page = select_page(&events, 0, &filter, 12);
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.cursor)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(page.cursor, 3);
    }

    #[test]
    fn project_and_agent_filters_use_context_retained_on_the_event() {
        let mut recorded = event(1, MonitoringEventKind::NeedsInput);
        recorded.project = Some("original-project".into());
        recorded.agent = Some("codex".into());
        let filter = EventFilter {
            project: Some("original-project".into()),
            agent: Some("codex".into()),
            ..EventFilter::default()
        };
        let recorded = [recorded];
        let page = select_page(&recorded, 0, &filter, 12);
        assert_eq!(page.events.len(), 1);
    }

    #[test]
    fn compact_context_is_repeated_only_after_a_change() {
        let mut first = event(1, MonitoringEventKind::Error);
        first.project = Some("workspace".into());
        first.title = Some("worker".into());
        let mut second = first.clone();
        second.cursor = 2;
        let mut renamed = second.clone();
        renamed.cursor = 3;
        renamed.title = Some("renamed".into());
        let rendered = compact_page(
            &[first, second, renamed],
            0,
            &EventFilter::default(),
            "epoch",
            12,
            4000,
        )
        .unwrap();
        assert_eq!(rendered.text.matches("workspace/worker").count(), 1);
        assert_eq!(rendered.text.matches("workspace/renamed").count(), 1);
    }

    #[test]
    fn compact_output_respects_the_character_budget_without_losing_events() {
        let events = vec![
            event(1, MonitoringEventKind::Error),
            event(2, MonitoringEventKind::Error),
            event(3, MonitoringEventKind::Error),
        ];
        let rendered = compact_page(&events, 0, &EventFilter::default(), "epoch", 12, 90).unwrap();
        assert!(rendered.text.chars().count() <= 90);
        assert!(rendered.text.contains("+"));
        assert!(rendered.omitted > 0);
        assert!(rendered.cursor < 3);
        let remaining = select_page(&events, rendered.cursor, &EventFilter::default(), 12);
        assert_eq!(remaining.events.len(), rendered.omitted);
    }

    #[test]
    fn compact_large_page_reports_all_omitted_matches_at_a_safe_cursor() {
        let events = (1..=4096)
            .map(|cursor| event(cursor, MonitoringEventKind::Error))
            .collect::<Vec<_>>();
        let page = compact_page(&events, 0, &EventFilter::default(), "epoch", 4096, 50).unwrap();
        assert!(page.text.chars().count() <= 50);
        assert_eq!(page.omitted, 4096);
        assert_eq!(page.cursor, 0);
        assert!(page.text.contains("+4096 more"));
    }

    #[test]
    fn compact_output_refuses_a_budget_too_small_to_report_overflow() {
        let events = vec![event(1, MonitoringEventKind::Error)];
        assert!(compact_page(&events, 0, &EventFilter::default(), "epoch", 12, 1).is_err());
    }

    #[test]
    fn compact_budget_counts_unicode_characters() {
        let mut unicode = event(1, MonitoringEventKind::NeedsInput);
        unicode.summary = "é".repeat(40);
        let rendered =
            compact_page(&[unicode], 0, &EventFilter::default(), "epoch", 12, 105).unwrap();
        assert!(rendered.text.chars().count() <= 105);
        assert_eq!(rendered.omitted, 0);
    }

    #[test]
    fn quiet_compact_result_contains_only_its_cursor() {
        let rendered = compact_page(&[], 0, &EventFilter::default(), "epoch", 12, 80).unwrap();
        assert_eq!(rendered.text, "quiet cursor=epoch:0\n");
        assert_eq!(rendered.cursor, 0);
        assert_eq!(rendered.omitted, 0);
    }

    fn digest_query() -> MonitoringQuery {
        MonitoringQuery {
            view: ilium_ipc::MonitoringView::Digest,
            since: None,
            consumer: Some("digest-manager".into()),
            follow: false,
            min_severity: None,
            kinds: Vec::new(),
            project: None,
            agent: None,
            pane_id: None,
            max_items: 1,
            max_chars: 4000,
            format: MonitoringFormat::Compact,
        }
    }

    #[tokio::test]
    async fn digest_acknowledgement_keeps_older_omissions_and_suppresses_delivered_evidence() {
        let events = crate::state::ServerEvents::new();
        events.record_monitoring(
            Some(NodeId(1)),
            MonitoringEventKind::Done,
            "fixture final reply",
        );
        events.record_monitoring(
            Some(NodeId(2)),
            MonitoringEventKind::NeedsInput,
            "fixture question",
        );
        let mut query = digest_query();
        query.format = MonitoringFormat::Json;
        let first = read_query(&events, &query).await.unwrap();
        assert!(first.contains("needs-you"));
        assert!(!first.contains("fixture final reply"));
        let epoch = events.monitoring.lock().unwrap().epoch();
        let first: serde_json::Value = serde_json::from_str(&first).unwrap();
        let first_cursor = first["cursor"].as_str().unwrap();
        assert_eq!(parse_cursor(first_cursor, &epoch.to_string()).unwrap(), 0);
        acknowledge(&events, "digest-manager", first_cursor).unwrap();
        let second = read_query(&events, &query).await.unwrap();
        assert!(second.contains("fixture final reply"));
        assert!(!second.contains("fixture question"));
        let second: serde_json::Value = serde_json::from_str(&second).unwrap();
        acknowledge(
            &events,
            "digest-manager",
            second["cursor"].as_str().unwrap(),
        )
        .unwrap();
        events.record_monitoring(
            Some(NodeId(1)),
            MonitoringEventKind::Done,
            "fixture final reply",
        );
        let quiet: serde_json::Value =
            serde_json::from_str(&read_query(&events, &query).await.unwrap()).unwrap();
        assert_eq!(quiet["quiet"], true);
    }

    #[tokio::test]
    async fn digest_json_reports_budgeted_overflow_and_a_safe_cursor() {
        let events = crate::state::ServerEvents::new();
        for pane in 1..=10 {
            events.record_monitoring(
                Some(NodeId(pane)),
                MonitoringEventKind::Error,
                &"é\\\"".repeat(30),
            );
        }
        let mut query = digest_query();
        query.format = MonitoringFormat::Json;
        query.max_chars = 160;
        let output = read_query(&events, &query).await.unwrap();
        assert!(output.chars().count() <= 160);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(value["more"].as_u64().unwrap() > 0);
        assert_eq!(
            parse_cursor(
                value["cursor"].as_str().unwrap(),
                &events.monitoring.lock().unwrap().epoch().to_string()
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn cursors_from_another_server_lifetime_are_rejected() {
        assert_eq!(parse_cursor("epoch:2", "epoch"), Ok(2));
        assert!(parse_cursor("old-epoch:2", "epoch").is_err());
        assert!(parse_cursor("epoch:not-a-number", "epoch").is_err());
    }
}
