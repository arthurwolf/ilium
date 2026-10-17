//! Twins of ilium-client/src/session_stats_store.rs: the per-pane statistics cache, its throttled refresh and the
//! bounded IO worker. Scenario ops `store.<name-with-dashes>` carry the case parameters and answer the key
//! observable values the original asserted (see ../API_GAPS_ports.md).

use serde_json::{json, Value};

use crate::common::client;

fn scenario(name: &str, request: Value) -> Value {
    client(&format!("store.{name}"), request)
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::a_missing_transcript_reports_unavailable
#[test]
fn port_session_stats_store_a_missing_transcript_reports_unavailable() {
    let reply = scenario(
        "a-missing-transcript-reports-unavailable",
        json!({ "transcript": "missing" }),
    );
    assert_eq!(reply["state"], json!("unavailable"));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::a_pass_reads_the_transcript_and_a_second_pass_is_throttled
#[test]
fn port_session_stats_store_a_pass_reads_the_transcript_and_a_second_pass_is_throttled() {
    let reply = scenario(
        "a-pass-reads-the-transcript-and-a-second-pass-is-throttled",
        json!({ "transcript": { "prompts": 1, "output_tokens": 9 }, "second_request_after_secs": 1, "third_request_after": "refresh_interval" }),
    );
    assert_eq!(reply["first_request_accepted"], json!(true));
    assert_eq!(reply["state"], json!("ready"));
    assert_eq!(reply["prompt_count"], json!(1));
    assert_eq!(reply["tokens"]["output"], json!(9));
    assert_eq!(
        reply["second_request_accepted"],
        json!(false),
        "a refresh inside the interval is skipped"
    );
    assert_eq!(reply["third_request_accepted"], json!(true));
    assert_eq!(reply["final_state"], json!("ready"));
    assert_eq!(
        reply["worker_joined"],
        json!(true),
        "isolated statistics IO worker joined"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::claude_subagent_files_count_toward_the_session_and_are_read_incrementally
#[test]
fn port_session_stats_store_claude_subagent_files_count_toward_the_session_and_are_read_incrementally(
) {
    let reply = scenario(
        "claude-subagent-files-count-toward-the-session-and-are-read-incrementally",
        json!({ "layout": "<session>/subagents/workflows/wf_1/agent-a1.jsonl", "initial_output": 1000, "appended_output": 500, "main_output": 9 }),
    );
    assert_eq!(
        reply["files"].as_array().map(Vec::len),
        Some(2),
        "main file plus the sub-agent"
    );
    assert_eq!(reply["output_after_first_pass"], json!(9 + 1000));
    assert_eq!(reply["output_after_growth"], json!(9 + 1000 + 500));
    assert_eq!(
        reply["worker_joined"],
        json!(true),
        "isolated statistics IO worker joined"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::claude_subagent_inventory_streams_more_than_4096_transcripts
#[test]
fn port_session_stats_store_claude_subagent_inventory_streams_more_than_4096_transcripts() {
    let reply = scenario(
        "claude-subagent-inventory-streams-more-than-4096-transcripts",
        json!({ "subagent_files": 4200 }),
    );
    assert_eq!(reply["seen"], json!(4_200));
    assert_eq!(reply["errors"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::claude_subagent_offsets_use_the_memory_budget_not_a_4096_file_cutoff
#[test]
fn port_session_stats_store_claude_subagent_offsets_use_the_memory_budget_not_a_4096_file_cutoff() {
    let reply = scenario(
        "claude-subagent-offsets-use-the-memory-budget-not-a-4096-file-cutoff",
        json!({ "files": 5000 }),
    );
    assert!(reply["retained_bytes"]
        .as_u64()
        .is_some_and(|bytes| bytes < 8 * 1024 * 1024));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::codex_session_lookup_succeeds_after_more_than_4096_unrelated_files
#[test]
fn port_session_stats_store_codex_session_lookup_succeeds_after_more_than_4096_unrelated_files() {
    let reply = scenario(
        "codex-session-lookup-succeeds-after-more-than-4096-unrelated-files",
        json!({ "unrelated_files": 4200, "agent": "codex" }),
    );
    assert_eq!(reply["request_accepted"], json!(true));
    assert_eq!(reply["state"], json!("ready"));
    assert_eq!(
        reply["worker_joined"],
        json!(true),
        "isolated statistics IO worker joined"
    );
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::queued_cancelled_pass_keeps_its_slot_until_real_worker_receipt
#[test]
fn port_session_stats_store_queued_cancelled_pass_keeps_its_slot_until_real_worker_receipt() {
    let reply = scenario(
        "queued-cancelled-pass-keeps-its-slot-until-real-worker-receipt",
        json!({ "io_threads": 1, "queue_slots": 4, "block_worker": true, "cancel_queued": true }),
    );
    assert_eq!(
        reply["request_returned_within_ms"]
            .as_u64()
            .map(|ms| ms < 1000),
        Some(true)
    );
    assert_eq!(reply["in_flight_after_request"], json!(1));
    assert_eq!(
        reply["in_flight_after_cancel"],
        json!(1),
        "cancel request cannot pretend the IO owner already retired"
    );
    assert_eq!(reply["in_flight_after_receipt"], json!(0));
    assert_eq!(reply["entry_exists"], json!(false));
    assert_eq!(reply["remaining_workers"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::returning_to_same_identity_rejects_worker_from_earlier_generation
#[test]
fn port_session_stats_store_returning_to_same_identity_rejects_worker_from_earlier_generation() {
    let reply = scenario(
        "returning-to-same-identity-rejects-worker-from-earlier-generation",
        json!({ "identity_cycle": ["a", "b", "a"], "late_events": ["progress", "failed", "finished"] }),
    );
    assert_eq!(reply["late_applied"], json!([false, false, false]));
    assert_eq!(reply["entry_source_unchanged"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::same_historical_transcript_keeps_exact_cached_snapshot
#[test]
fn port_session_stats_store_same_historical_transcript_keeps_exact_cached_snapshot() {
    let reply = scenario(
        "same-historical-transcript-keeps-exact-cached-snapshot",
        json!({ "reconcile_with_same_context": true }),
    );
    assert_eq!(reply["reconcile_changed"], json!(false));
    assert_eq!(reply["snapshot_same_arc"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::same_session_string_in_another_owner_must_not_reuse_prior_stats
#[test]
fn port_session_stats_store_same_session_string_in_another_owner_must_not_reuse_prior_stats() {
    let reply = scenario(
        "same-session-string-in-another-owner-must-not-reuse-prior-stats",
        json!({ "session_id": "same", "replacement_owner": "other" }),
    );
    assert_eq!(reply["matching_entry"], Value::Null);
    assert_eq!(reply["request_accepted"], json!(true));
    assert_eq!(
        reply["old_snapshot_visible"],
        json!(false),
        "old owner snapshot must be hidden immediately"
    );
    assert_eq!(reply["transcript_path"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::twelve_installed_statistics_release_the_single_finite_job_slot
#[test]
fn port_session_stats_store_twelve_installed_statistics_release_the_single_finite_job_slot() {
    let reply = scenario(
        "twelve-installed-statistics-release-the-single-finite-job-slot",
        json!({ "panes": 12, "queue_slots": 1, "resident_mib_per_thread": 1 }),
    );
    assert_eq!(reply["entries"], json!(12));
    assert_eq!(reply["states"], json!(vec!["ready"; 12]));
    assert_eq!(reply["prompt_counts"], json!(vec![1; 12]));
    assert_eq!(reply["output_tokens"], json!(vec![9; 12]));
    assert_eq!(
        reply["jobs_while_installed"],
        json!(0),
        "installed cache must not pin the single job credit"
    );
    assert_eq!(reply["input_bytes"], json!(0));
    assert_eq!(reply["result_bytes"], json!(0));
    assert_eq!(reply["jobs_after_shutdown"], json!(0));
    assert_eq!(reply["worker_threads_after_shutdown"], json!(0));
    assert_eq!(reply["exported_source_survives_producer"], json!(true));
    assert_eq!(reply["worker_bytes_after_shutdown"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats_store.rs::unresolved_replacement_drops_inflight_cache_and_all_late_result_kinds
#[test]
fn port_session_stats_store_unresolved_replacement_drops_inflight_cache_and_all_late_result_kinds()
{
    let reply = scenario(
        "unresolved-replacement-drops-inflight-cache-and-all-late-result-kinds",
        json!({ "reconcile_with": "empty", "late_events": ["progress", "failed", "finished"] }),
    );
    assert_eq!(reply["reconcile_changed"], json!(true));
    assert_eq!(reply["drain_changed"], json!(false));
    assert_eq!(reply["entry_exists"], json!(false));
}
