//! Priority-sorted, bounded monitoring digests with acknowledgement-safe paging.

use ilium_core::attention::{fingerprint_parts, AttentionKind};
use ilium_core::NodeId;
use ilium_ipc::{MonitoringEvent, MonitoringEventKind};
use std::collections::HashMap;

pub struct DigestPage {
    pub text: String,
    pub cursor: u64,
    pub omitted: usize,
    pub delivered: Vec<(NodeId, u64)>,
    pub contexts: Vec<(NodeId, u64)>,
}

pub struct DigestOptions<'a> {
    pub after: u64,
    pub epoch: &'a str,
    pub working: usize,
    pub max_items: usize,
    pub max_chars: usize,
    pub contexts: &'a HashMap<NodeId, u64>,
    pub acknowledged: &'a HashMap<NodeId, u64>,
}

pub fn render_digest(
    events: &[MonitoringEvent],
    options: &DigestOptions<'_>,
) -> Result<DigestPage, &'static str> {
    if options.max_items == 0
        || options.max_items > 4096
        || options.max_chars == 0
        || options.max_chars > 1_000_000
    {
        return Err("invalid output budget");
    }
    // A later state supersedes a former question or failure for the same pane.
    // Progress-only updates do not erase actionable state.
    let mut latest = HashMap::new();
    let mut cursor = options.after;
    for event in events.iter().filter(|event| event.cursor > options.after) {
        cursor = cursor.max(event.cursor);
        if matches!(
            event.kind,
            MonitoringEventKind::Progress | MonitoringEventKind::ResourceChanged
        ) {
            continue;
        }
        if let Some(pane) = event.pane_id {
            let previous: &mut &MonitoringEvent = latest.entry(pane).or_insert(event);
            let lifecycle = matches!(
                event.kind,
                MonitoringEventKind::Started
                    | MonitoringEventKind::Idle
                    | MonitoringEventKind::Done
                    | MonitoringEventKind::Closed
            );
            let rank = |kind| match kind {
                MonitoringEventKind::NeedsInput => 0,
                MonitoringEventKind::Error | MonitoringEventKind::Stuck => 1,
                _ => 2,
            };
            let previous_lifecycle = matches!(
                previous.kind,
                MonitoringEventKind::Started
                    | MonitoringEventKind::Idle
                    | MonitoringEventKind::Done
                    | MonitoringEventKind::Closed
            );
            if event.cursor > previous.cursor
                && (lifecycle || previous_lifecycle || rank(event.kind) <= rank(previous.kind))
            {
                *previous = event;
            }
        }
    }
    let mut items: Vec<_> = latest
        .into_iter()
        .filter_map(|(pane, event)| {
            let (priority, group) = match event.kind {
                MonitoringEventKind::NeedsInput => (AttentionKind::Question, "needs-you"),
                MonitoringEventKind::Error | MonitoringEventKind::Stuck => {
                    (AttentionKind::Stuck, "failed-or-stuck")
                }
                MonitoringEventKind::Done => (AttentionKind::Changed, "finished"),
                _ => return None,
            };
            let fingerprint = fingerprint_parts(&[
                &event.generation.to_le_bytes(),
                event.kind.as_str().as_bytes(),
                event.summary.as_bytes(),
                event.project.as_deref().unwrap_or("").as_bytes(),
                event.title.as_deref().unwrap_or("").as_bytes(),
                event.agent.as_deref().unwrap_or("").as_bytes(),
            ]);
            if options.acknowledged.get(&pane) == Some(&fingerprint) {
                return None;
            }
            let context = if options.contexts.get(&pane) == Some(&context_fingerprint(event)) {
                "-".to_owned()
            } else {
                [event.project.as_deref(), event.title.as_deref()]
                    .into_iter()
                    .flatten()
                    .map(clean_text)
                    .collect::<Vec<_>>()
                    .join("/")
            };
            let summary = clean_text(&event.summary);
            let line = format!(
                "{group} {} {context} {summary} -> ilium ctl pane {} --brief\n",
                pane.0, pane.0
            );
            Some((
                priority,
                pane,
                event.cursor,
                fingerprint,
                line,
                context_fingerprint(event),
            ))
        })
        .collect();
    items.sort_by_key(|(priority, pane, event_cursor, _, _, _)| (*priority, *event_cursor, *pane));
    let limit = items.len().min(options.max_items);
    let mut prefix_chars = vec![0];
    for item in items.iter().take(limit) {
        prefix_chars.push(prefix_chars.last().copied().unwrap_or(0) + item.4.chars().count());
    }
    for count in (0..=limit).rev() {
        let omitted = items.len() - count;
        let page_cursor = items[count..]
            .iter()
            .map(|item| item.2.saturating_sub(1))
            .min()
            .unwrap_or(cursor)
            .max(options.after);
        let encoded = format!("{}:{page_cursor}", options.epoch);
        let footer = if omitted > 0 {
            format!("+{omitted} more (cursor {encoded})\n")
        } else if count == 0 {
            format!("quiet cursor={encoded}\n")
        } else {
            format!("working={} cursor={encoded}\n", options.working)
        };
        if prefix_chars[count] + footer.chars().count() > options.max_chars {
            continue;
        }
        let mut text = String::new();
        for item in items.iter().take(count) {
            text.push_str(&item.4);
        }
        text.push_str(&footer);
        return Ok(DigestPage {
            text,
            cursor: page_cursor,
            omitted,
            contexts: items
                .iter()
                .take(count)
                .map(|item| (item.1, item.5))
                .collect(),
            delivered: items
                .iter()
                .take(count)
                .map(|item| (item.1, item.3))
                .collect(),
        });
    }
    Err("output budget is too small to report a cursor and overflow")
}

pub fn context_fingerprint(event: &MonitoringEvent) -> u64 {
    fingerprint_parts(&[
        event.project.as_deref().unwrap_or("").as_bytes(),
        event.title.as_deref().unwrap_or("").as_bytes(),
        event.agent.as_deref().unwrap_or("").as_bytes(),
    ])
}

fn clean_text(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ipc::MonitoringEventKind;

    fn options(acknowledged: &HashMap<NodeId, u64>) -> DigestOptions<'_> {
        DigestOptions {
            after: 0,
            epoch: "e",
            working: 0,
            max_items: 1,
            max_chars: 4000,
            contexts: acknowledged,
            acknowledged,
        }
    }

    fn event(cursor: u64, kind: MonitoringEventKind, summary: &str) -> MonitoringEvent {
        MonitoringEvent {
            cursor,
            generation: 0,
            pane_id: Some(NodeId(cursor)),
            kind,
            project: Some("fixture".into()),
            title: Some("worker".into()),
            agent: Some("codex".into()),
            observed_at_unix_millis: 0,
            summary: summary.into(),
        }
    }

    #[test]
    fn review_a_question_remains_visible_alongside_a_lower_priority_signal() {
        let question = event(1, MonitoringEventKind::NeedsInput, "Which target?");
        let mut stuck = event(2, MonitoringEventKind::Stuck, "Idle resource holder");
        stuck.pane_id = question.pane_id;
        stuck.title = Some("newer undisplayed context".into());
        let question_context = (question.pane_id.unwrap(), context_fingerprint(&question));
        let page = render_digest(&[question, stuck], &options(&HashMap::new())).unwrap();
        assert!(page.text.starts_with("needs-you"), "{}", page.text);
        assert!(page.text.contains("Which target?"));
        assert_eq!(page.contexts, vec![question_context]);
        assert!(!page.text.contains("newer undisplayed context"));
    }

    #[test]
    fn severity_ordered_pages_do_not_lose_older_omitted_events_or_repeat_acknowledged_items() {
        let events = vec![
            event(1, MonitoringEventKind::Done, "delivered fixture result"),
            event(2, MonitoringEventKind::Error, "fixture failure"),
            event(3, MonitoringEventKind::NeedsInput, "fixture question"),
        ];
        let mut acknowledged = HashMap::new();
        let page = render_digest(&events, &options(&acknowledged)).unwrap();
        assert!(page.text.contains("needs-you"));
        assert!(!page.text.contains("fixture failure"));
        assert_eq!((page.cursor, page.omitted), (0, 2));
        acknowledged.extend(page.delivered);
        let page = render_digest(&events, &options(&acknowledged)).unwrap();
        assert!(page.text.contains("failed-or-stuck"));
        assert!(!page.text.contains("fixture question"));
        assert_eq!((page.cursor, page.omitted), (0, 1));
        acknowledged.extend(page.delivered);
        let page = render_digest(&events, &options(&acknowledged)).unwrap();
        assert!(page.text.contains("finished"));
        assert!(page.text.contains("delivered fixture result"));
        assert_eq!((page.cursor, page.omitted), (3, 0));
    }

    #[test]
    fn repeated_evidence_is_suppressed_but_new_evidence_reappears() {
        let first = event(1, MonitoringEventKind::Error, "fixture failure");
        let mut acknowledged = HashMap::new();
        let page = render_digest(std::slice::from_ref(&first), &options(&acknowledged)).unwrap();
        acknowledged.extend(page.delivered);
        let mut repeated = first;
        repeated.cursor = 2;
        assert!(render_digest(&[repeated.clone()], &options(&acknowledged))
            .unwrap()
            .text
            .starts_with("quiet "));
        repeated.summary = "different fixture failure".into();
        assert!(render_digest(&[repeated], &options(&acknowledged))
            .unwrap()
            .text
            .contains("different fixture failure"));
    }

    #[test]
    fn generation_change_reopens_identical_evidence_from_a_new_work_phase() {
        let mut event = event(1, MonitoringEventKind::Done, "fixture passed");
        event.generation = 1;
        let mut acknowledged = HashMap::new();
        acknowledged.extend(
            render_digest(&[event.clone()], &options(&acknowledged))
                .unwrap()
                .delivered,
        );
        event.cursor = 3;
        event.generation = 2;
        assert!(render_digest(&[event], &options(&acknowledged))
            .unwrap()
            .text
            .contains("fixture passed"));
    }

    #[test]
    fn simulated_two_hundred_pane_digest_meets_quiet_and_ten_change_byte_budgets() {
        let acknowledged = HashMap::new();
        let options = DigestOptions {
            after: 0,
            epoch: "00000000-0000-0000-0000-000000000000",
            working: 200,
            max_items: 12,
            max_chars: 4000,
            contexts: &HashMap::new(),
            acknowledged: &acknowledged,
        };
        let quiet = render_digest(&[], &options).unwrap();
        assert!(quiet.text.starts_with("quiet "));
        assert!(quiet.text.len() <= 80);
        let events: Vec<_> = (1..=10)
            .map(|id| event(id, MonitoringEventKind::Done, "simulated fixture result"))
            .collect();
        let changed = render_digest(&events, &options).unwrap();
        assert!(changed.text.len() <= 2500);
        assert_eq!(changed.delivered.len(), 10);
        assert_eq!(changed.omitted, 0);
    }

    #[test]
    fn small_unicode_output_budgets_report_overflow_without_skipping_unseen_items() {
        let acknowledged = HashMap::new();
        let events = vec![event(1, MonitoringEventKind::Error, &"é".repeat(120))];
        let options = DigestOptions {
            after: 0,
            epoch: "e",
            working: 2,
            max_items: 12,
            max_chars: 60,
            contexts: &HashMap::new(),
            acknowledged: &acknowledged,
        };
        let page = render_digest(&events, &options).unwrap();
        assert!(page.text.chars().count() <= 60);
        assert!(page.text.contains("+1 more"));
        assert_eq!((page.cursor, page.omitted), (0, 1));
        assert!(page.delivered.is_empty());
    }
}
