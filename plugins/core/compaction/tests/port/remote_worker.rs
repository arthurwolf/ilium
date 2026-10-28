//! Twins of the inline tests in ilium-client/src/remote_compaction_worker.rs (worker error
//! classification, pause waiting and cancellation). Gaps: `worker_annotate_progress_overflow`,
//! `worker_classify_inference_error`, `worker_await_pause`, `worker_pause_probe_cost`,
//! `worker_start_pause_job`, `worker_fill_event_queue`, `worker_cancel`, `worker_cancel_observed`
//! (see ../API_GAPS_port.md).

use std::time::Duration;

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::progress_overflow_never_rewrites_a_committed_success_into_failure
#[test]
fn rwrk_progress_overflow_never_rewrites_a_committed_success_into_failure() {
    let host = client_host();
    assert_eq!(host.worker_annotate_progress_overflow(Ok(json!(42)), true), Ok(json!(42)),
        "a completed transcript rewrite remains successful even if a final progress event was dropped");
    assert_eq!(
        host.worker_annotate_progress_overflow(Err("compaction cancelled".to_string()), true),
        Err("Remote compaction stopped because its UI event queue was full".to_string())
    );
    assert_eq!(
        host.worker_annotate_progress_overflow(
            Err("transcript verification failed".to_string()),
            true
        ),
        Err("transcript verification failed".to_string()),
        "progress overload must not hide the actual domain failure"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::payload_too_large_asks_for_a_smaller_chunk
#[test]
fn rwrk_payload_too_large_asks_for_a_smaller_chunk() {
    let host = client_host();
    assert_eq!(
        host.worker_classify_inference_error(
            json!({"http": {"status": 413, "message": "payload too large"}})
        ),
        "context-too-long"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::bad_request_about_context_length_asks_for_a_smaller_chunk
#[test]
fn rwrk_bad_request_about_context_length_asks_for_a_smaller_chunk() {
    let host = client_host();
    let error = json!({"http": {"status": 400, "message": "This model's maximum context length is 128000 tokens"}});
    assert_eq!(
        host.worker_classify_inference_error(error),
        "context-too-long"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::other_failures_stay_plain_failures
#[test]
fn rwrk_other_failures_stay_plain_failures() {
    let host = client_host();
    for error in [
        json!({"http": {"status": 429, "message": "rate limited"}}),
        json!({"http": {"status": 400, "message": "invalid api key"}}),
        json!({"transport": "connection reset"}),
    ] {
        assert_eq!(host.worker_classify_inference_error(error), "failed");
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_wait_returns_when_cancelled
#[test]
fn rwrk_pause_wait_returns_when_cancelled() {
    let host = client_host();
    let outcome = host.worker_await_pause(
        "claude",
        "/nonexistent/transcript.jsonl",
        Duration::from_secs(60),
        Duration::from_secs(60),
        true,
    );
    assert_eq!(outcome.result, Err("cancelled".to_string()));
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_wait_requests_one_interrupt_then_gives_up
#[test]
fn rwrk_pause_wait_requests_one_interrupt_then_gives_up() {
    let host = client_host();
    let outcome = host.worker_await_pause(
        "claude",
        "/nonexistent/transcript.jsonl",
        Duration::ZERO,
        Duration::from_millis(600),
        false,
    );
    assert_eq!(outcome.interrupts, 1);
    // manual does not pin the give-up message; it must at least be a real failure, not a cancel
    let message = outcome.result.expect_err("the wait gives up");
    assert!(!message.is_empty());
    assert_ne!(message, "cancelled");
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_probe_admission_covers_tail_parsing_peak
#[test]
fn rwrk_pause_probe_admission_covers_tail_parsing_peak() {
    let host = client_host();
    assert!(host.worker_pause_probe_cost_input_bytes() >= 64 * 1024 * 1024,
        "pause detection parses up to a 4 MiB JSONL tail, so its admitted peak must cover parsing overhead");
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::cancellation_completes_even_when_the_ui_event_queue_is_full
#[test]
fn rwrk_cancellation_completes_even_when_the_ui_event_queue_is_full() {
    let mut host = client_host();
    host.worker_fill_event_queue();
    host.worker_start_pause_job(
        "claude",
        "/nonexistent/remote-compaction-transcript",
        Duration::ZERO,
    );
    assert!(
        host.worker_wait_interrupt_pending(Duration::from_secs(2)),
        "the pause job attempts its interrupt"
    );
    host.worker_cancel();
    assert!(
        host.worker_cancel_observed(Duration::from_secs(2)),
        "cancellation completes with a full queue"
    );
    assert!(
        host.worker_idle(Duration::from_secs(2)),
        "no job stays active after cancellation"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::progress_overflow_never_rewrites_a_committed_success_into_failure
#[test]
fn rwrk_progress_overflow_keeps_a_failure_unchanged_when_the_queue_did_not_overflow() {
    let host = client_host();
    assert_eq!(
        host.worker_annotate_progress_overflow(Err("compaction cancelled".to_string()), false),
        Err("compaction cancelled".to_string())
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::payload_too_large_asks_for_a_smaller_chunk
#[test]
fn rwrk_payload_too_large_is_context_too_long_whatever_the_message() {
    let host = client_host();
    assert_eq!(
        host.worker_classify_inference_error(json!({"http": {"status": 413, "message": ""}})),
        "context-too-long"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::bad_request_about_context_length_asks_for_a_smaller_chunk
#[test]
fn rwrk_bad_request_saying_the_prompt_is_too_long_asks_for_a_smaller_chunk() {
    let host = client_host();
    assert_eq!(
        host.worker_classify_inference_error(
            json!({"http": {"status": 400, "message": "prompt is too long"}})
        ),
        "context-too-long"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::other_failures_stay_plain_failures
#[test]
fn rwrk_server_error_stays_a_plain_failure() {
    let host = client_host();
    assert_eq!(
        host.worker_classify_inference_error(json!({"http": {"status": 500, "message": "internal"}})),
        "failed"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_wait_returns_when_cancelled
#[test]
fn rwrk_cancelled_pause_wait_sends_no_interrupt() {
    let host = client_host();
    let outcome = host.worker_await_pause(
        "claude",
        "/nonexistent/transcript.jsonl",
        Duration::from_secs(60),
        Duration::from_secs(60),
        true,
    );
    assert_eq!(outcome.interrupts, 0);
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_wait_requests_one_interrupt_then_gives_up
#[test]
fn rwrk_pause_wait_never_sends_more_than_one_interrupt() {
    let host = client_host();
    let outcome = host.worker_await_pause(
        "claude",
        "/nonexistent/transcript.jsonl",
        Duration::ZERO,
        Duration::from_millis(600),
        true,
    );
    assert!(outcome.interrupts <= 1);
    assert!(outcome.result.is_err());
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_probe_admission_covers_tail_parsing_peak
#[test]
fn rwrk_pause_probe_admission_is_bounded() {
    let host = client_host();
    assert!(host.worker_pause_probe_cost_input_bytes() <= 1024 * 1024 * 1024);
}

/// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::cancellation_completes_even_when_the_ui_event_queue_is_full
#[test]
fn rwrk_repeated_cancel_is_harmless() {
    let mut host = client_host();
    host.worker_fill_event_queue();
    host.worker_start_pause_job(
        "claude",
        "/nonexistent/remote-compaction-transcript",
        Duration::ZERO,
    );
    host.worker_cancel();
    host.worker_cancel();
    assert!(host.worker_cancel_observed(Duration::from_secs(2)));
}
