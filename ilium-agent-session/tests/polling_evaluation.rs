//! A small labelled synthetic replay for the local candidate filter.
//! Candidate selection is deliberately not a final polling verdict.
use ilium_agent_session::polling_activity::candidate_signal;
use ilium_core::polling_detection::{ActivityEvent, ActivityKind};

fn event(kind: ActivityKind, name: Option<&str>, text: &str) -> ActivityEvent {
    ActivityEvent {
        kind,
        timestamp: None,
        name: name.map(str::to_owned),
        call_id: None,
        text: text.to_owned(),
        truncated: false,
        is_error: None,
    }
}

#[test]
fn labelled_candidate_replay_includes_waits_updates_and_independent_work() {
    let call = |text| event(ActivityKind::ToolCall, Some("exec_command"), text);
    let message = |text| event(ActivityKind::Message, None, text);
    let wait = |text| event(ActivityKind::ToolCall, Some("wait"), text);
    let cases = [
        (
            "repeated status",
            vec![
                call("job status 1"),
                call("job status 1"),
                call("job status 1"),
            ],
            true,
        ),
        (
            "repeated updates",
            vec![
                message("The job is still running"),
                message("The job is still running"),
                message("The job is still running"),
            ],
            true,
        ),
        (
            "wait continuation",
            vec![
                wait(r#"{"cell_id":"a","yield_time_ms":1000}"#),
                wait(r#"{"cell_id":"a","yield_time_ms":5000}"#),
                wait(r#"{"cell_id":"a","yield_time_ms":30000}"#),
            ],
            true,
        ),
        (
            "single blocking wait",
            vec![wait(r#"{"cell_id":"a","yield_time_ms":300000}"#)],
            false,
        ),
        (
            "independent work",
            vec![call("read module"), call("edit module"), call("run tests")],
            false,
        ),
        (
            "distinct waits",
            vec![
                wait(r#"{"cell_id":"a"}"#),
                wait(r#"{"cell_id":"b"}"#),
                wait(r#"{"cell_id":"c"}"#),
            ],
            false,
        ),
    ];
    let mut true_positives = 0;
    let mut false_positives = 0;
    let mut false_negatives = 0;
    for (name, events, expected) in cases {
        let actual = candidate_signal(&events).is_some();
        match (actual, expected) {
            (true, true) => true_positives += 1,
            (true, false) => false_positives += 1,
            (false, true) => false_negatives += 1,
            (false, false) => {}
        }
        assert_eq!(actual, expected, "synthetic replay: {name}");
    }
    eprintln!("synthetic local candidates: true positives={true_positives}, false positives={false_positives}, false negatives={false_negatives}; this is not a calibrated provider evaluation");
}

#[test]
fn changing_results_can_still_reach_the_advisory_model() {
    let call = || event(ActivityKind::ToolCall, Some("exec_command"), "job status 1");
    let result = |text| event(ActivityKind::ToolResult, None, text);
    let events = vec![
        call(),
        result("running 10%"),
        call(),
        result("running 50%"),
        call(),
        result("completed"),
    ];
    // This is a known extra candidate, not a polling verdict: the filter is
    // intentionally permissive and downstream classification must see change.
    assert!(candidate_signal(&events).is_some());
}

/// Synthetic, clearly labelled corpus used to choose the default evidence
/// window. Positive traces dilute the repeated behaviour among unrelated work
/// at different spacings; negative traces are legitimate patterns. Nothing here
/// is a real transcript, so the numbers rank window sizes against each other
/// and say nothing about production accuracy.
fn diluted(
    repeated: impl Fn() -> ActivityEvent,
    every: usize,
    repetitions: usize,
) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for round in 0..repetitions {
        events.push(repeated());
        for step in 1..every {
            events.push(event(
                ActivityKind::ToolCall,
                Some("exec_command"),
                &format!("edit module part {round}-{step}"),
            ));
        }
    }
    events
}

#[test]
fn window_size_replay_ranks_candidate_recall_against_token_cost() {
    let status = || event(ActivityKind::ToolCall, Some("exec_command"), "job status 1");
    let update = || event(ActivityKind::Message, None, "The job is still running");
    let wait = || {
        event(
            ActivityKind::ToolCall,
            Some("wait"),
            r#"{"cell_id":"a","yield_time_ms":5000}"#,
        )
    };
    let work = |count: usize| -> Vec<ActivityEvent> {
        (0..count)
            .map(|index| {
                event(
                    ActivityKind::ToolCall,
                    Some("exec_command"),
                    &format!("edit module {index}"),
                )
            })
            .collect()
    };
    let positives = [
        ("status, dense", diluted(status, 1, 3)),
        ("updates, dense", diluted(update, 1, 3)),
        ("wait churn, dense", diluted(wait, 1, 3)),
        ("status every 5th event", diluted(status, 5, 3)),
        ("updates every 6th event", diluted(update, 6, 3)),
        ("status every 9th event", diluted(status, 9, 3)),
    ];
    let negatives = [
        ("independent work", work(40)),
        ("one status among work", diluted(status, 12, 1)),
        (
            "distinct waits among work",
            vec![
                event(ActivityKind::ToolCall, Some("wait"), r#"{"cell_id":"a"}"#),
                event(ActivityKind::ToolCall, Some("wait"), r#"{"cell_id":"b"}"#),
                event(ActivityKind::ToolCall, Some("wait"), r#"{"cell_id":"c"}"#),
            ],
        ),
    ];
    let mut recalls = Vec::new();
    for window in [10_usize, 15, 30] {
        let mut found = 0;
        let mut false_positives = 0;
        let mut sent_tokens = 0_usize;
        for (_, events) in &positives {
            let start = events.len().saturating_sub(window);
            let tail = &events[start..];
            if candidate_signal(tail).is_some() {
                found += 1;
                sent_tokens += 600 + tail.iter().map(|e| e.text.len()).sum::<usize>() / 4;
            }
        }
        for (_, events) in &negatives {
            let start = events.len().saturating_sub(window);
            let tail = &events[start..];
            if candidate_signal(tail).is_some() {
                false_positives += 1;
                sent_tokens += 600 + tail.iter().map(|e| e.text.len()).sum::<usize>() / 4;
            }
        }
        let sent = found + false_positives;
        eprintln!(
            "window {window:>2}: recall {found}/{} precision {found}/{sent} approx tokens per replay {sent_tokens}",
            positives.len()
        );
        recalls.push(found);
    }
    assert!(
        recalls.windows(2).all(|pair| pair[0] <= pair[1]),
        "a wider window never loses a repeated pattern: {recalls:?}"
    );
}
