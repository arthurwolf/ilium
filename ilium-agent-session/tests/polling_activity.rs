use ilium_agent_session::polling_activity::ActivityExtractor;
use ilium_core::polling_detection::{ActivityEvent, ActivityKind};
use ilium_core::AgentClass;
use serde_json::json;

#[test]
fn codex_records_keep_messages_calls_and_linked_results_in_order() {
    let mut extractor = ActivityExtractor::default();
    let events = [
        json!({"type":"response_item","timestamp":"2026-11-15T13:00:00Z","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"Still waiting"}]}}),
        json!({"type":"response_item","timestamp":"2026-11-15T13:00:01Z","payload":{"type":"function_call","name":"wait","call_id":"call-1","arguments":"{\"cell_id\":\"189\",\"yield_time_ms\":20000}"}}),
        json!({"type":"response_item","timestamp":"2026-11-15T13:00:21Z","payload":{"type":"function_call_output","call_id":"call-1","output":"Script running"}}),
    ];
    let found: Vec<ActivityEvent> = events
        .iter()
        .flat_map(|entry| extractor.observe(&AgentClass::Codex, entry))
        .collect();
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].kind, ActivityKind::Message);
    assert_eq!(found[1].kind, ActivityKind::ToolCall);
    assert_eq!(found[2].kind, ActivityKind::ToolResult);
    assert_eq!(found[1].call_id, found[2].call_id);
    assert!(!found[2].counts_for_interval());
}

#[test]
fn claude_deduplicates_repeated_assistant_blocks_and_ignores_user_prompts() {
    let mut extractor = ActivityExtractor::default();
    let assistant = json!({"type":"assistant","message":{"id":"message-1","content":[{"type":"text","text":"Checking"},{"type":"tool_use","id":"tool-1","name":"Bash","input":{"command":"ilium progress wait 1"}}]}});
    assert_eq!(extractor.observe(&AgentClass::Claude, &assistant).len(), 2);
    assert!(extractor
        .observe(&AgentClass::Claude, &assistant)
        .is_empty());
    assert!(extractor
        .observe(
            &AgentClass::Claude,
            &json!({"type":"user","message":{"content":"hello"}})
        )
        .is_empty());
}

#[test]
fn claude_wait_arguments_keep_a_bounded_handle_and_drop_unrelated_fields() {
    let mut extractor = ActivityExtractor::default();
    let entry = json!({"type":"assistant","message":{"id":"m","content":[{"type":"tool_use","id":"t","name":"wait","input":{"cell_id":"cell-1","yield_time_ms":20000,"unrelated_secret":"must-not-appear"}}]}});
    let found = extractor.observe(&AgentClass::Claude, &entry);
    assert!(found[0].text.contains("cell-1"));
    assert!(!found[0].text.contains("must-not-appear"));
}

#[test]
fn claude_wait_continuations_group_by_handle_when_duration_changes() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    let calls: Vec<_> = [1000, 5000, 30000].into_iter().enumerate().flat_map(|(index, duration)| {
        extractor.observe(&AgentClass::Claude, &json!({"type":"assistant","message":{"id":format!("m{index}"),"content":[{"type":"tool_use","id":format!("t{index}"),"name":"wait","input":{"cell_id":"same","yield_time_ms":duration}}]}}))
    }).collect();
    assert_eq!(
        candidate_signal(&calls),
        Some(CandidateSignal::WaitContinuation)
    );
}

#[test]
fn claude_unrelated_structured_arguments_do_not_create_a_shared_target() {
    use ilium_agent_session::polling_activity::candidate_signal;
    let mut extractor = ActivityExtractor::default();
    let calls: Vec<_> = (0..3).flat_map(|index| {
        extractor.observe(&AgentClass::Claude, &json!({"type":"assistant","message":{"id":format!("m{index}"),"content":[{"type":"tool_use","id":format!("t{index}"),"name":"read","input":{"file_path":format!("file-{index}")}}]}}))
    }).collect();
    assert_eq!(candidate_signal(&calls), None);
}

#[test]
fn claude_structured_tool_results_remain_linked_and_bounded() {
    let mut extractor = ActivityExtractor::default();
    let call = json!({"type":"assistant","message":{"id":"m","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"job status"}}]}});
    extractor.observe(&AgentClass::Claude, &call);
    let result = json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"text","text":"pending"}]}]}});
    let events = extractor.observe(&AgentClass::Claude, &result);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "pending");
    assert_eq!(events[0].call_id.as_deref(), Some("t"));
}

#[test]
fn unsupported_and_sidechain_transcripts_do_not_produce_events() {
    let mut extractor = ActivityExtractor::default();
    let entry = json!({"type":"assistant","isSidechain":true,"message":{"id":"m","content":[{"type":"text","text":"x"}]}});
    assert!(extractor.observe(&AgentClass::Claude, &entry).is_empty());
    assert!(extractor
        .observe(&AgentClass::Other("example".into()), &entry)
        .is_empty());
}

#[test]
fn large_activity_fields_are_bounded_without_breaking_unicode() {
    let mut extractor = ActivityExtractor::default();
    let entry = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"é".repeat(5000)}]}});
    let events = extractor.observe(&AgentClass::Codex, &entry);
    assert_eq!(events.len(), 1);
    assert!(events[0].text.chars().count() <= 200);
    assert!(events[0].truncated);
    let exact = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"é".repeat(200)}]}});
    let events = extractor.observe(&AgentClass::Codex, &exact);
    assert!(!events[0].truncated);
}

#[test]
fn codex_does_not_count_reasoning_or_user_messages() {
    let mut extractor = ActivityExtractor::default();
    for entry in [
        json!({"type":"response_item","payload":{"type":"reasoning","summary":[]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}),
    ] {
        assert!(extractor.observe(&AgentClass::Codex, &entry).is_empty());
    }
}

#[test]
fn codex_replayed_message_id_counts_only_once() {
    let mut extractor = ActivityExtractor::default();
    let entry = json!({"type":"response_item","payload":{"type":"message","id":"message-1","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"Waiting"}]}});
    assert_eq!(extractor.observe(&AgentClass::Codex, &entry).len(), 1);
    assert!(extractor.observe(&AgentClass::Codex, &entry).is_empty());
}

#[test]
fn codex_assistant_analysis_is_not_user_visible_activity() {
    let mut extractor = ActivityExtractor::default();
    let entry = json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"analysis","content":[{"type":"output_text","text":"private reasoning"}]}});
    assert!(extractor.observe(&AgentClass::Codex, &entry).is_empty());
    let entry = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"analysis","content":[{"type":"output_text","text":"private reasoning"}]}});
    assert!(extractor.observe(&AgentClass::Codex, &entry).is_empty());
}

#[test]
fn tool_results_without_a_captured_call_are_not_retained() {
    let mut extractor = ActivityExtractor::default();
    let orphan = json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"missing","output":"private output"}});
    assert!(extractor.observe(&AgentClass::Codex, &orphan).is_empty());
}

#[test]
fn activity_window_counts_calls_and_messages_but_not_results() {
    use ilium_agent_session::polling_activity::ActivityWindow;
    let mut extractor = ActivityExtractor::default();
    let mut window = ActivityWindow::new(5, 15);
    let msg = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"Waiting"}]}});
    let result = json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"x","output":"pending"}});
    for _ in 0..4 {
        for event in extractor.observe(&AgentClass::Codex, &msg) {
            assert!(!window.push(event));
        }
    }
    for event in extractor.observe(&AgentClass::Codex, &result) {
        assert!(!window.push(event));
    }
    for event in extractor.observe(&AgentClass::Codex, &msg) {
        assert!(window.push(event));
    }
    assert_eq!(window.eligible_count(), 5);
}

#[test]
fn interval_and_spacing_coalesce_activity_without_a_catch_up_burst() {
    use ilium_agent_session::polling_activity::ActivityWindow;
    use std::time::Duration;
    let mut window = ActivityWindow::new(2, 15);
    let event = ActivityEvent {
        kind: ActivityKind::Message,
        timestamp: None,
        name: None,
        call_id: None,
        text: "waiting".into(),
        truncated: false,
        is_error: None,
    };
    let spacing = Duration::from_secs(60);
    window.push(event.clone());
    assert!(!window.ready_at(Duration::from_secs(100), spacing));
    window.push(event.clone());
    assert!(window.ready_at(Duration::from_secs(100), spacing));
    window.mark_checked_at(Duration::from_secs(100));
    for _ in 0..10 {
        window.push(event.clone());
    }
    assert!(!window.ready_at(Duration::from_secs(99), spacing));
    assert!(!window.ready_at(Duration::from_secs(159), spacing));
    assert!(window.ready_at(Duration::from_secs(160), spacing));
    window.mark_checked_at(Duration::from_secs(160));
    assert!(!window.ready_at(Duration::from_secs(1000), spacing));
}

#[test]
fn candidate_filter_distinguishes_reissued_checks_from_one_blocking_wait() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    let checks: Vec<_> = (0..3).flat_map(|i| extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":format!("c{i}"),"arguments":"{\"cmd\":\"job status 42\"}"}}))).collect();
    assert_eq!(
        candidate_signal(&checks),
        Some(CandidateSignal::RepeatedCheck)
    );
    let wait = extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","name":"wait","call_id":"w","arguments":"{\"cell_id\":\"42\",\"yield_time_ms\":300000}"}}));
    assert_eq!(candidate_signal(&wait), None);
}

#[test]
fn warned_pattern_of_repeated_wait_continuations_is_a_candidate_not_a_polling_verdict() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    // Synthetic shape of the reviewed iutube trace: the same wait handle is
    // continued repeatedly. This alone does not prove new underlying checks.
    let events: Vec<_> = (0..3).flat_map(|index| extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","name":"wait","call_id":format!("w{index}"),"arguments":"{\"cell_id\":\"same\",\"yield_time_ms\":20000}"}}))).collect();
    assert_eq!(
        candidate_signal(&events),
        Some(CandidateSignal::WaitContinuation)
    );
    let namespaced: Vec<_> = (0..3).flat_map(|index| extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","name":"mcp__terminal__wait","call_id":format!("n{index}"),"arguments":"{\"cell_id\":\"same\"}"}}))).collect();
    assert_eq!(
        candidate_signal(&namespaced),
        Some(CandidateSignal::WaitContinuation)
    );
}

#[test]
fn wait_continuations_keep_their_identity_when_requested_durations_change() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    let calls: Vec<_> = [1000, 5000, 30000].into_iter().enumerate().flat_map(|(index, duration)| {
        let arguments = json!({"session_id": 42, "yield_time_ms": duration}).to_string();
        extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","name":"write_stdin","call_id":format!("w{index}"),"arguments":arguments}}))
    }).collect();
    assert_eq!(
        candidate_signal(&calls),
        Some(CandidateSignal::WaitContinuation)
    );
    let mut different = calls.clone();
    different[2].text = json!({"session_id": 43, "yield_time_ms": 30000}).to_string();
    assert_eq!(candidate_signal(&different), None);
    for (index, call) in different.iter_mut().enumerate() {
        call.text = json!({"yield_time_ms": (index + 1) * 1000}).to_string();
    }
    assert_eq!(candidate_signal(&different), None);
}

#[test]
fn repeated_truncated_tool_prefixes_are_uncertain_candidates() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let event = ActivityEvent {
        kind: ActivityKind::ToolCall,
        timestamp: None,
        name: Some("exec_command".into()),
        call_id: None,
        text: "status ".into(),
        truncated: true,
        is_error: None,
    };
    assert_eq!(
        candidate_signal(&[event.clone(), event.clone(), event]),
        Some(CandidateSignal::RepeatedCheck)
    );
}

#[test]
fn repeated_unchanged_messages_are_a_separate_candidate() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    let message = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"The job is still running."}]}});
    let events: Vec<_> = (0..3)
        .flat_map(|_| extractor.observe(&AgentClass::Codex, &message))
        .collect();
    assert_eq!(
        candidate_signal(&events),
        Some(CandidateSignal::RepeatedUpdate)
    );
}

#[test]
fn minor_wording_changes_in_waiting_updates_are_candidates() {
    use ilium_agent_session::polling_activity::{candidate_signal, CandidateSignal};
    let mut extractor = ActivityExtractor::default();
    let messages = [
        "The build job is still running",
        "The build job is still running now",
        "The build job is still running remotely",
    ];
    let events: Vec<_> = messages.into_iter().flat_map(|message| extractor.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":message}]}}))).collect();
    assert_eq!(
        candidate_signal(&events),
        Some(CandidateSignal::RepeatedUpdate)
    );
}

#[test]
fn framer_retains_partial_records_and_marks_oversized_gaps() {
    use ilium_agent_session::polling_activity::{ActivityFramer, FramedRecord};
    let mut framer = ActivityFramer::new(8);
    assert!(framer.push(b"abc").is_empty());
    assert_eq!(
        framer.push(b"d\n0123456789\nok\n"),
        vec![
            FramedRecord::Record(b"abcd".to_vec()),
            FramedRecord::Oversized,
            FramedRecord::Record(b"ok".to_vec())
        ]
    );
}
