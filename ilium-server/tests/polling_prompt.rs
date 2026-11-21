use ilium_core::polling_detection::{ActivityEvent, ActivityKind};
use ilium_server::polling_prompt::compact_evidence;

fn event(kind: ActivityKind, text: &str) -> ActivityEvent {
    ActivityEvent {
        kind,
        timestamp: None,
        name: None,
        call_id: None,
        text: text.into(),
        truncated: false,
        is_error: None,
    }
}

#[test]
fn prompt_omits_raw_outputs_and_redacts_credential_patterns() {
    let events = vec![
        event(
            ActivityKind::Message,
            "waiting with sk-abcdefghijklmnopqrstuv",
        ),
        event(ActivityKind::ToolResult, "pending private output"),
        event(ActivityKind::ToolCall, "cat .env"),
    ];
    let prompt = compact_evidence(&events, "known-key", 3000).unwrap();
    assert!(!prompt.contains("sk-abcdefghijklmnopqrstuv"));
    assert!(!prompt.contains("private output"));
    assert!(!prompt.contains("cat .env"));
    assert!(prompt.contains("pending"));
}

#[test]
fn prompt_preserves_valid_json_under_a_tight_limit() {
    let events: Vec<_> = (0..100)
        .map(|index| {
            event(
                ActivityKind::Message,
                &format!("{index}: {}", "é".repeat(200)),
            )
        })
        .collect();
    let prompt = compact_evidence(&events, "key", 500).unwrap();
    assert!(prompt.len() <= 500);
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert!(parsed["omitted_events"].as_u64().unwrap() > 0);
}

#[test]
fn prompt_links_repeated_commands_and_results_without_exposing_arguments() {
    let mut first = event(ActivityKind::ToolCall, "status private-job-a");
    first.call_id = Some("call-a".into());
    let mut result = event(ActivityKind::ToolResult, "pending private data");
    result.call_id = Some("call-a".into());
    let repeated = event(ActivityKind::ToolCall, "status private-job-a");
    let different = event(ActivityKind::ToolCall, "status private-job-b");
    let prompt = compact_evidence(&[first, result, repeated, different], "", 3000).unwrap();
    assert!(!prompt.contains("private-job"));
    assert!(!prompt.contains("private data"));
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let events = parsed["events"].as_array().unwrap();
    assert_eq!(events[0]["target"], events[1]["target"]);
    assert_eq!(events[0]["target"], events[2]["target"]);
    assert_ne!(events[0]["target"], events[3]["target"]);
}

#[test]
fn truncated_commands_are_not_assumed_to_have_the_same_target() {
    let mut truncated = event(ActivityKind::ToolCall, "status shared-prefix");
    truncated.truncated = true;
    let prompt = compact_evidence(&[truncated], "", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert!(parsed["events"][0]["target"].is_null());
}

#[test]
fn negated_and_completed_results_are_not_reported_as_pending() {
    let events = [
        event(ActivityKind::ToolResult, "process is not running"),
        event(ActivityKind::ToolResult, "running task completed"),
    ];
    let prompt = compact_evidence(&events, "", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert_eq!(parsed["events"][0]["text"], "unknown");
    assert_eq!(parsed["events"][1]["text"], "completed");
}

#[test]
fn wait_grouping_ignores_requested_duration_but_preserves_distinct_sessions() {
    let mut first = event(
        ActivityKind::ToolCall,
        r#"{"session_id":1,"yield_time_ms":1000}"#,
    );
    first.name = Some("write_stdin".into());
    let mut continued = event(
        ActivityKind::ToolCall,
        r#"{"session_id":1,"yield_time_ms":5000}"#,
    );
    continued.name = first.name.clone();
    let mut other = event(
        ActivityKind::ToolCall,
        r#"{"session_id":2,"yield_time_ms":5000}"#,
    );
    other.name = first.name.clone();
    let prompt = compact_evidence(&[first, continued, other], "", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let events = parsed["events"].as_array().unwrap();
    assert_eq!(events[0]["target"], events[1]["target"]);
    assert_ne!(events[0]["target"], events[2]["target"]);
    assert_eq!(events[0]["requested_wait_ms"], 1000);
    assert_eq!(events[1]["requested_wait_ms"], 5000);
    assert!(!prompt.contains("session_id"));
}

#[test]
fn claude_wait_evidence_keeps_durations_and_hides_the_wait_handle() {
    use ilium_agent_session::polling_activity::ActivityExtractor;
    use ilium_core::AgentClass;
    let mut extractor = ActivityExtractor::default();
    let events: Vec<_> = [1000, 5000]
        .into_iter()
        .enumerate()
        .flat_map(|(index, duration)| {
            extractor.observe(
                &AgentClass::Claude,
                &serde_json::json!({
                    "type":"assistant", "message":{"id":format!("m{index}"),"content":[{
                        "type":"tool_use", "id":format!("t{index}"), "name":"wait",
                        "input":{"cell_id":"private-handle", "yield_time_ms":duration}
                    }]}
                }),
            )
        })
        .collect();
    let prompt = compact_evidence(&events, "", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert_eq!(parsed["events"][0]["target"], parsed["events"][1]["target"]);
    assert_eq!(parsed["events"][0]["requested_wait_ms"], 1000);
    assert_eq!(parsed["events"][1]["requested_wait_ms"], 5000);
    assert!(!prompt.contains("private-handle"));
}

#[test]
fn changing_results_are_marked_without_disclosing_raw_output() {
    let mut first_call = event(ActivityKind::ToolCall, "status private-job");
    first_call.call_id = Some("one".into());
    let mut first_result = event(ActivityKind::ToolResult, "running private-data 10%");
    first_result.call_id = Some("one".into());
    let mut second_call = event(ActivityKind::ToolCall, "status private-job");
    second_call.call_id = Some("two".into());
    let mut second_result = event(ActivityKind::ToolResult, "running private-data 50%");
    second_result.call_id = Some("two".into());
    let prompt = compact_evidence(
        &[first_call, first_result, second_call, second_result],
        "",
        3000,
    )
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert!(parsed["events"][1]["changed_since_last"].is_null());
    assert_eq!(parsed["events"][3]["changed_since_last"], true);
    assert!(!prompt.contains("private-data"));
    assert!(!prompt.contains("private-job"));
}

#[test]
fn truncated_results_do_not_claim_equality_or_change() {
    let mut call_one = event(ActivityKind::ToolCall, "status job");
    call_one.call_id = Some("one".into());
    let mut result_one = event(ActivityKind::ToolResult, "running");
    result_one.call_id = Some("one".into());
    result_one.truncated = true;
    let mut call_two = event(ActivityKind::ToolCall, "status job");
    call_two.call_id = Some("two".into());
    let mut result_two = event(ActivityKind::ToolResult, "running");
    result_two.call_id = Some("two".into());
    result_two.truncated = true;
    let prompt = compact_evidence(&[call_one, result_one, call_two, result_two], "", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert!(parsed["events"][3]["changed_since_last"].is_null());
}

#[test]
fn identical_results_are_only_compared_with_the_same_target() {
    let mut call_one = event(ActivityKind::ToolCall, "status job-a");
    call_one.call_id = Some("one".into());
    let mut result_one = event(ActivityKind::ToolResult, "pending");
    result_one.call_id = Some("one".into());
    let mut call_two = event(ActivityKind::ToolCall, "status job-a");
    call_two.call_id = Some("two".into());
    let mut result_two = event(ActivityKind::ToolResult, "pending");
    result_two.call_id = Some("two".into());
    let mut call_three = event(ActivityKind::ToolCall, "status job-b");
    call_three.call_id = Some("three".into());
    let mut result_three = event(ActivityKind::ToolResult, "pending");
    result_three.call_id = Some("three".into());
    let prompt = compact_evidence(
        &[
            call_one,
            result_one,
            call_two,
            result_two,
            call_three,
            result_three,
        ],
        "",
        3000,
    )
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert_eq!(parsed["events"][3]["changed_since_last"], false);
    assert!(parsed["events"][5]["changed_since_last"].is_null());
}

fn repeated_wait_cycles() -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for index in 0..3 {
        let call_id = format!("synthetic-call-{index}");
        let timestamp = format!("2026-11-16T12:00:0{index}Z");
        let mut call = event(
            ActivityKind::ToolCall,
            r#"{"cell_id":"synthetic-build","yield_time_ms":300000}"#,
        );
        call.name = Some("wait".into());
        call.call_id = Some(call_id.clone());
        call.timestamp = Some(timestamp.clone());
        let mut result = event(ActivityKind::ToolResult, "pending");
        result.call_id = Some(call_id);
        result.timestamp = Some(timestamp.clone());
        let mut message = event(ActivityKind::Message, "The build is still running.");
        message.timestamp = Some(timestamp);
        events.extend([call, result, message]);
    }
    events
}

#[test]
fn repeated_wait_runs_preserve_order_outcomes_commentary_and_observed_timing() {
    let prompt = compact_evidence(&repeated_wait_cycles(), "synthetic-key", 12_288).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let runs = parsed["events"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run["kind"], "run");
    assert_eq!(run["repetitions"], 3);
    assert_eq!(run["events"][0]["kind"], "tool_call");
    assert_eq!(run["events"][1]["kind"], "tool_result");
    assert_eq!(run["events"][2]["kind"], "message");
    assert_eq!(run["last_events"][1]["text"], "pending");
    assert_eq!(run["first_timestamp"], "2026-11-16T12:00:00Z");
    assert_eq!(run["last_timestamp"], "2026-11-16T12:00:02Z");
    assert_eq!(run["elapsed_ms"], 2000);
    assert_eq!(run["minimum_interval_ms"], 1000);
    assert_eq!(run["maximum_interval_ms"], 1000);
    assert_eq!(run["commentary_between"], true);
    assert_eq!(parsed["omitted_events"], 0);
}

#[test]
fn changed_results_and_intervening_work_are_not_compacted_into_unchanged_wait_runs() {
    let mut changed = repeated_wait_cycles();
    changed[4].text = "completed".into();
    let mut work = repeated_wait_cycles();
    work.insert(3, event(ActivityKind::ToolCall, "edit module"));
    for events in [changed, work] {
        let prompt = compact_evidence(&events, "synthetic-key", 12_288).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert!(parsed["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["kind"] != "run" || entry["repetitions"] != 3));
    }
}

#[test]
fn runs_with_unknown_timestamps_do_not_invent_intervals() {
    let mut events = repeated_wait_cycles();
    for event in &mut events {
        event.timestamp = None;
    }
    let prompt = compact_evidence(&events, "synthetic-key", 12_288).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let run = &parsed["events"][0];
    assert_eq!(run["repetitions"], 3);
    assert!(run["elapsed_ms"].is_null());
    assert!(run["minimum_interval_ms"].is_null());
    assert!(run["maximum_interval_ms"].is_null());
}

#[test]
fn a_wait_run_retains_requested_duration_changes_for_the_same_target() {
    let mut events = repeated_wait_cycles();
    events[0].text = r#"{"cell_id":"synthetic-build","yield_time_ms":1000}"#.into();
    events[3].text = r#"{"cell_id":"synthetic-build","yield_time_ms":30000}"#.into();
    let prompt = compact_evidence(&events, "synthetic-key", 12_288).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let run = &parsed["events"][0];
    assert_eq!(run["repetitions"], 3);
    assert_eq!(run["events"][0]["requested_wait_ms"], 1000);
    assert_eq!(run["last_events"][0]["requested_wait_ms"], 300000);
    assert_eq!(run["minimum_requested_wait_ms"], 1000);
    assert_eq!(run["maximum_requested_wait_ms"], 300000);
}

#[test]
fn a_nonmonotonic_wait_run_does_not_invent_clock_intervals() {
    let mut events = repeated_wait_cycles();
    for event in &mut events[3..6] {
        event.timestamp = Some("2026-11-16T11:59:59Z".into());
    }
    let prompt = compact_evidence(&events, "synthetic-key", 12_288).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    let run = &parsed["events"][0];
    assert_eq!(run["repetitions"], 3);
    assert!(run["elapsed_ms"].is_null());
    assert!(run["minimum_interval_ms"].is_null());
    assert!(run["maximum_interval_ms"].is_null());
}

#[test]
fn scripts_containing_status_text_are_not_presented_as_status_queries() {
    let mut call = event(
        ActivityKind::ToolCall,
        r#"{"cmd":"python3 -c 'print(\"status of the edited module\")'"}"#,
    );
    call.name = Some("exec_command".into());
    let prompt = compact_evidence(&[call], "synthetic-key", 3000).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&prompt).unwrap();
    assert_eq!(parsed["events"][0]["text"], "other tool call");
}
