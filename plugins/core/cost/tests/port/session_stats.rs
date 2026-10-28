//! Twins of ilium-client/src/session_stats.rs: the transcript accumulator (Claude Code and Codex records), token
//! totals, prompts, tools, quota buckets and timeline folding. Scenario ops `stats.<name-with-dashes>` take the case
//! description (the originals read JSONL files, so the twins carry the key values and fixture description) and
//! answer the snapshot fields the original asserted (see ../API_GAPS_ports.md).

use serde_json::{json, Value};

use crate::common::client;

fn scenario(name: &str, request: Value) -> Value {
    client(&format!("stats.{name}"), request)
}

/// REQ: PORT:ilium-client/src/session_stats.rs::a_truncated_file_restarts_the_accumulator
#[test]
fn port_session_stats_a_truncated_file_restarts_the_accumulator() {
    let reply = scenario(
        "a-truncated-file-restarts-the-accumulator",
        json!({ "agent": "claude", "files": [["msg_1:40 plus a corrupt tail"], ["msg_9:7"]] }),
    );
    assert_eq!(reply["tokens"]["output"], json!(7));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::cache_hit_ratio_is_none_without_prompt_tokens
#[test]
fn port_session_stats_cache_hit_ratio_is_none_without_prompt_tokens() {
    let reply = scenario(
        "cache-hit-ratio-is-none-without-prompt-tokens",
        json!({ "cases": [{}, { "input": 10, "cache_read": 90 }] }),
    );
    assert_eq!(reply["ratios"][0], Value::Null);
    assert_eq!(reply["ratios"][1], json!(0.9));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::cancellation_preserves_the_last_complete_record_offset
#[test]
fn port_session_stats_cancellation_preserves_the_last_complete_record_offset() {
    let reply = scenario(
        "cancellation-preserves-the-last-complete-record-offset",
        json!({ "agent": "codex", "file": "{}\n", "cancel_first": true }),
    );
    assert!(reply["first_error"]
        .as_str()
        .unwrap_or("")
        .contains("cancelled"));
    assert_eq!(reply["offset_after_cancel"], json!(0));
    assert_eq!(reply["offset_after_resume"], json!(3));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_counts_only_human_prompts_and_skips_harness_records
#[test]
fn port_session_stats_claude_counts_only_human_prompts_and_skips_harness_records() {
    let reply = scenario(
        "claude-counts-only-human-prompts-and-skips-harness-records",
        json!({ "agent": "claude", "lines": ["human:fix the login bug", "task-notification", "meta", "sidechain", "local-command-echo", "tool_result is_error"] }),
    );
    assert_eq!(reply["prompt_count"], json!(1));
    assert_eq!(reply["recent_prompts"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        reply["recent_prompts"][0]["text"],
        json!("fix the login bug")
    );
    assert_eq!(reply["tool_errors"], json!(1));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_merges_repeated_blocks_of_one_response_instead_of_summing
#[test]
fn port_session_stats_claude_merges_repeated_blocks_of_one_response_instead_of_summing() {
    let reply = scenario(
        "claude-merges-repeated-blocks-of-one-response-instead-of-summing",
        json!({ "agent": "claude", "responses": [["msg_1", 40, "Bash"], ["msg_1", 40, "Bash"], ["msg_2", 60, null]] }),
    );
    assert_eq!(reply["tokens"]["output"], json!(100));
    assert_eq!(reply["tokens"]["input"], json!(6));
    assert_eq!(reply["tokens"]["cache_read"], json!(2000));
    assert_eq!(reply["tokens"]["cache_write"], json!(200));
    assert_eq!(reply["tokens"]["reasoning"], json!(10));
    assert_eq!(reply["model_calls"], json!(2));
    assert_eq!(
        reply["tool_calls"],
        json!(1),
        "the repeated tool_use id counts once"
    );
    assert_eq!(reply["tools"], json!([["Bash", 1]]));
    assert_eq!(reply["model"], json!("claude-sonnet-5"));
    assert_eq!(reply["effort"], json!("high"));
    assert_eq!(reply["git_branch"], json!("main"));
    assert_eq!(reply["context_tokens"], json!(1103));
    assert_eq!(reply["span_ms"], json!(30_000));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_records_turn_durations_and_cost_snapshot
#[test]
fn port_session_stats_claude_records_turn_durations_and_cost_snapshot() {
    let reply = scenario(
        "claude-records-turn-durations-and-cost-snapshot",
        json!({ "agent": "claude", "turn_durations_ms": [32500, 1500], "cost_state": { "totalCostUSD": 1.25, "totalAPIDuration": 9000, "totalToolDuration": 3000, "totalLinesAdded": 10 } }),
    );
    assert_eq!(reply["turns_completed"], json!(2));
    assert_eq!(reply["active_ms"], json!(34_000));
    assert_eq!(reply["longest_turn_ms"], json!(32_500));
    assert!(
        reply["cost"]["total_usd"]
            .as_f64()
            .is_some_and(|usd| (usd - 1.25).abs() < 1e-9),
        "the cost snapshot is retained"
    );
    assert_eq!(reply["cost"]["model_costs"][0][0], json!("claude-sonnet-5"));
    assert_eq!(reply["cost"]["lines_added"], json!(10));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_slash_commands_read_as_typed
#[test]
fn port_session_stats_claude_slash_commands_read_as_typed() {
    let reply = scenario(
        "claude-slash-commands-read-as-typed",
        json!({ "agent": "claude", "commands": [["/goal", "goal", "ship it"], ["/compact", null, null]] }),
    );
    assert_eq!(reply["recent_prompts"][0]["text"], json!("/goal ship it"));
    assert_eq!(reply["recent_prompts"][1]["text"], json!("/compact"));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_subagent_tokens_count_toward_spend_but_not_context
#[test]
fn port_session_stats_claude_subagent_tokens_count_toward_spend_but_not_context() {
    let reply = scenario(
        "claude-subagent-tokens-count-toward-spend-but-not-context",
        json!({ "agent": "claude", "responses": [["msg_a", 10, false], ["msg_b", 500, true]] }),
    );
    assert_eq!(reply["tokens"]["output"], json!(510));
    assert_eq!(reply["model_calls"], json!(1));
    assert_eq!(reply["samples"].as_array().map(Vec::len), Some(1));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_synthetic_model_counts_an_api_error_without_tokens
#[test]
fn port_session_stats_claude_synthetic_model_counts_an_api_error_without_tokens() {
    let reply = scenario(
        "claude-synthetic-model-counts-an-api-error-without-tokens",
        json!({ "agent": "claude", "model": "<synthetic>", "text": "API Error" }),
    );
    assert_eq!(reply["api_errors"], json!(1));
    assert_eq!(reply["tokens_total"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_transcript_yields_work_marks_and_progress_runs
#[test]
fn port_session_stats_claude_transcript_yields_work_marks_and_progress_runs() {
    let reply = scenario(
        "claude-transcript-yields-work-marks-and-progress-runs",
        json!({ "agent": "claude", "fixture": "build-it transcript with a failed progress monitor 4" }),
    );
    assert!(!reply["work_marks"].is_null(), "work marks are produced");
    assert_eq!(reply["progress_spans"].as_array().map(Vec::len), Some(1));
    assert_eq!(reply["progress_spans"][0]["outcome"], json!("failure"));
    assert_eq!(reply["progress_span_ends_after_start"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_falls_back_to_event_prompts_when_no_response_prompts_exist
#[test]
fn port_session_stats_codex_falls_back_to_event_prompts_when_no_response_prompts_exist() {
    let reply = scenario(
        "codex-falls-back-to-event-prompts-when-no-response-prompts-exist",
        json!({ "agent": "codex", "event_prompts": ["only event"], "response_prompts": [] }),
    );
    assert_eq!(reply["recent_prompts"][0]["text"], json!("only event"));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_quota_rises_become_per_minute_consumption_and_resets_only_rebaseline
#[test]
fn port_session_stats_codex_quota_rises_become_per_minute_consumption_and_resets_only_rebaseline() {
    let reply = scenario(
        "codex-quota-rises-become-per-minute-consumption-and-resets-only-rebaseline",
        json!({ "agent": "codex", "fixture": "primary rises and a reset, flat secondary" }),
    );
    assert_eq!(reply["primary_buckets"], json!([3.5, 1.5, 1.0]));
    assert_eq!(
        reply["secondary_buckets"],
        json!([]),
        "a flat window consumed nothing"
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_tracks_tools_turns_and_prefers_response_item_prompts
#[test]
fn port_session_stats_codex_tracks_tools_turns_and_prefers_response_item_prompts() {
    let reply = scenario(
        "codex-tracks-tools-turns-and-prefers-response-item-prompts",
        json!({ "agent": "codex", "prompts": { "event": "event copy", "response": "real prompt" }, "calls": ["function_call", "custom_tool_call"], "outputs": ["custom_tool_call_output"], "task_complete": { "duration_ms": 192373 }, "compacted": 1 }),
    );
    assert_eq!(reply["prompt_count"], json!(1));
    assert_eq!(reply["recent_prompts"][0]["text"], json!("real prompt"));
    assert_eq!(reply["tool_calls"], json!(2), "outputs are not calls");
    assert_eq!(reply["turns_completed"], json!(1));
    assert_eq!(reply["active_ms"], json!(192_373));
    assert_eq!(reply["average_first_token_ms"], json!(5008));
    assert_eq!(reply["compactions"], json!(1));
    assert_eq!(reply["tool_errors"], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_uses_cumulative_totals_and_ignores_repeated_events
#[test]
fn port_session_stats_codex_uses_cumulative_totals_and_ignores_repeated_events() {
    let reply = scenario(
        "codex-uses-cumulative-totals-and-ignores-repeated-events",
        json!({ "agent": "codex", "fixture": "cumulative token_count events with a repeated event" }),
    );
    assert_eq!(
        reply["tokens"]["input"],
        json!(1600),
        "uncached input = input - cached"
    );
    assert_eq!(reply["tokens"]["cache_read"], json!(1400));
    assert_eq!(reply["tokens"]["output"], json!(150));
    assert_eq!(reply["tokens"]["reasoning"], json!(60));
    assert_eq!(reply["model_calls"], json!(2));
    assert_eq!(reply["context_tokens"], json!(2000));
    assert_eq!(reply["context_window"], json!(258_400));
    assert_eq!(reply["model"], json!("gpt-5.6-terra"));
    assert_eq!(reply["effort"], json!("medium"));
    assert_eq!(reply["plan_type"], json!("pro"));
    assert_eq!(reply["provider"], json!("OpenAI"));
    assert_eq!(reply["rate_limits"].as_array().map(Vec::len), Some(1));
    assert!(reply["rate_limits"][0][1]["used_percent"]
        .as_f64()
        .is_some_and(|used| (used - 74.0).abs() < 1e-9));
    assert_eq!(reply["models"][0]["calls"], json!(2));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::decimation_preserves_totals_and_peaks
#[test]
fn port_session_stats_decimation_preserves_totals_and_peaks() {
    let reply = scenario(
        "decimation-preserves-totals-and-peaks",
        json!({ "samples": "more than SAMPLE_CAP" }),
    );
    assert_eq!(reply["merged_within_cap"], json!(true));
    assert_eq!(reply["merged_output_total"], reply["expected_output_total"]);
    assert_eq!(reply["merged_peak_context"], reply["expected_peak_context"]);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::identical_consecutive_prompts_collapse_into_one_record_with_a_repeat_count
#[test]
fn port_session_stats_identical_consecutive_prompts_collapse_into_one_record_with_a_repeat_count() {
    let reply = scenario(
        "identical-consecutive-prompts-collapse-into-one-record-with-a-repeat-count",
        json!({ "agent": "codex", "prompts": ["keep going", "keep going", "stop"] }),
    );
    assert_eq!(reply["recent_prompts"].as_array().map(Vec::len), Some(2));
    assert_eq!(reply["recent_prompts"][0]["repeats"], json!(2));
    assert_eq!(reply["recent_prompts"][1]["repeats"], json!(1));
    assert_eq!(
        reply["prompt_count"],
        json!(3),
        "every send still counts as a prompt"
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::ingest_file_resumes_from_its_offset_and_skips_a_partial_last_line
#[test]
fn port_session_stats_ingest_file_resumes_from_its_offset_and_skips_a_partial_last_line() {
    let reply = scenario(
        "ingest-file-resumes-from-its-offset-and-skips-a-partial-last-line",
        json!({ "agent": "claude", "steps": ["msg_1:40 plus partial", "complete msg_1 and msg_2:60"] }),
    );
    assert_eq!(reply["output_after_first"], json!(40));
    assert_eq!(reply["output_after_second"], json!(100));
    assert_eq!(reply["model_calls"], json!(2));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::main_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines
#[test]
fn port_session_stats_main_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines() {
    let reply = scenario(
        "main-transcript-reads-are-bounded-per-pass-and-resume-without-losing-lines",
        json!({ "file": "two passes of lines", "kind": "main" }),
    );
    assert_eq!(reply["offset_after_first"], reply["pass_bytes"]);
    assert_eq!(
        reply["offset_after_second"].as_u64(),
        reply["pass_bytes"].as_u64().map(|bytes| 2 * bytes)
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::malformed_and_irrelevant_lines_are_ignored
#[test]
fn port_session_stats_malformed_and_irrelevant_lines_are_ignored() {
    let reply = scenario(
        "malformed-and-irrelevant-lines-are-ignored",
        json!({ "agent": "claude", "lines": ["truncated json", "queue-operation"], "timeline": "four buckets" }),
    );
    assert_eq!(reply["prompt_count"], json!(0));
    assert_eq!(reply["buckets"].as_array().map(Vec::len), Some(4));
    assert_eq!(reply["buckets"][0]["tokens"]["output"], json!(10));
    assert_eq!(reply["buckets"][3]["tokens"]["output"], json!(30));
    assert_eq!(reply["buckets"][3]["peak_context"], json!(9));
    assert_eq!(reply["buckets_event_total"], json!(3));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::oversized_complete_line_is_explicitly_incomplete_and_does_not_advance_offset
#[test]
fn port_session_stats_oversized_complete_line_is_explicitly_incomplete_and_does_not_advance_offset()
{
    let reply = scenario(
        "oversized-complete-line-is-explicitly-incomplete-and-does-not-advance-offset",
        json!({ "agent": "claude", "oversized_line": true }),
    );
    assert!(reply["error"]
        .as_str()
        .unwrap_or("")
        .contains("totals incomplete"));
    assert_eq!(reply["offset"], json!(0));
    assert_eq!(reply["file_unchanged"], json!(true));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::progress_outcomes_are_read_from_delivered_monitor_text
#[test]
fn port_session_stats_progress_outcomes_are_read_from_delivered_monitor_text() {
    let reply = scenario(
        "progress-outcomes-are-read-from-delivered-monitor-text",
        json!({ "texts": ["Ilium progress monitor 4 reports that job failed", "Ilium progress monitor 4 reports that job finished", "Ilium progress monitor 4 reports that job was cleared", "fix the progress monitor"] }),
    );
    assert_eq!(reply["outcomes"][0], json!("failure"));
    assert_eq!(reply["outcomes"][1], json!("success"));
    assert_eq!(reply["outcomes"][2], json!("cleared"));
    assert_eq!(reply["outcomes"][3], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::real_transcript_report
#[test]
#[ignore = "reads a real transcript named by ILIUM_STATS_SAMPLE"]
fn port_session_stats_real_transcript_report() {
    let reply = scenario(
        "real-transcript-report",
        json!({ "sample_env": "ILIUM_STATS_SAMPLE" }),
    );
    assert!(
        !reply.is_null(),
        "the report is produced for the sampled transcript"
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::sidecar_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines
#[test]
fn port_session_stats_sidecar_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines(
) {
    let reply = scenario(
        "sidecar-transcript-reads-are-bounded-per-pass-and-resume-without-losing-lines",
        json!({ "file": "two passes of lines", "kind": "sidecar" }),
    );
    assert_eq!(reply["offset_after_first"], reply["pass_bytes"]);
    assert_eq!(
        reply["offset_after_second"].as_u64(),
        reply["pass_bytes"].as_u64().map(|bytes| 2 * bytes)
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::timeline_buckets_fold_samples_and_activity
#[test]
fn port_session_stats_timeline_buckets_fold_samples_and_activity() {
    let reply = scenario(
        "timeline-buckets-fold-samples-and-activity",
        json!({ "samples": "three events over four buckets" }),
    );
    assert_eq!(reply["buckets"].as_array().map(Vec::len), Some(4));
    assert_eq!(reply["buckets"][0]["tokens"]["output"], json!(10));
    assert_eq!(reply["buckets"][3]["tokens"]["output"], json!(30));
    assert_eq!(reply["buckets"][3]["peak_context"], json!(9));
    assert_eq!(reply["buckets_event_total"], json!(3));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::tool_result_lines_are_never_parsed_but_failures_are_counted
#[test]
fn port_session_stats_tool_result_lines_are_never_parsed_but_failures_are_counted() {
    let reply = scenario(
        "tool-result-lines-are-never-parsed-but-failures-are-counted",
        json!({ "agent": "claude", "tool_result_bytes": 100000, "is_error": true }),
    );
    assert_eq!(reply["tool_errors"], json!(1));
    assert_eq!(reply["records_parsed"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::a_truncated_file_restarts_the_accumulator
#[test]
fn port_session_stats_truncated_file_restart_drops_the_earlier_model_call() {
    let reply = scenario(
        "a-truncated-file-restarts-the-accumulator",
        json!({ "agent": "claude", "files": [["msg_1:40 plus a corrupt tail"], ["msg_9:7"]] }),
    );
    assert_eq!(reply["model_calls"], json!(1));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::cache_hit_ratio_is_none_without_prompt_tokens
#[test]
fn port_session_stats_cache_hit_ratio_is_none_when_prompt_tokens_are_zero() {
    let reply = scenario(
        "cache-hit-ratio-is-none-without-prompt-tokens",
        json!({ "cases": [{ "input": 0, "cache_read": 0 }] }),
    );
    assert_eq!(reply["ratios"][0], Value::Null);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::cancellation_preserves_the_last_complete_record_offset
#[test]
fn port_session_stats_uncancelled_ingest_reads_the_complete_record() {
    let reply = scenario(
        "cancellation-preserves-the-last-complete-record-offset",
        json!({ "agent": "codex", "file": "{}\n", "cancel_first": false }),
    );
    assert_eq!(reply["first_error"], Value::Null);
    assert_eq!(reply["offset_after_resume"], json!(3));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_counts_only_human_prompts_and_skips_harness_records
#[test]
fn port_session_stats_harness_only_transcript_counts_no_prompts() {
    let reply = scenario(
        "claude-counts-only-human-prompts-and-skips-harness-records",
        json!({ "agent": "claude", "lines": ["task-notification", "meta", "sidechain"] }),
    );
    assert_eq!(reply["prompt_count"], json!(0));
    assert_eq!(reply["recent_prompts"].as_array().map(Vec::len), Some(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_merges_repeated_blocks_of_one_response_instead_of_summing
#[test]
fn port_session_stats_distinct_claude_response_ids_are_summed() {
    let reply = scenario(
        "claude-merges-repeated-blocks-of-one-response-instead-of-summing",
        json!({ "agent": "claude", "responses": [["msg_1", 40, null], ["msg_2", 60, null]] }),
    );
    assert_eq!(reply["tokens"]["output"], json!(100));
    assert_eq!(reply["model_calls"], json!(2));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_records_turn_durations_and_cost_snapshot
#[test]
fn port_session_stats_single_turn_duration_is_the_longest_turn() {
    let reply = scenario(
        "claude-records-turn-durations-and-cost-snapshot",
        json!({ "agent": "claude", "turn_durations_ms": [1500] }),
    );
    assert_eq!(reply["turns_completed"], json!(1));
    assert_eq!(reply["longest_turn_ms"], json!(1500));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_slash_commands_read_as_typed
#[test]
fn port_session_stats_slash_command_arguments_are_appended_to_the_command() {
    let reply = scenario(
        "claude-slash-commands-read-as-typed",
        json!({ "agent": "claude", "commands": [["/model", "model", "claude-sonnet-5"]] }),
    );
    assert_eq!(
        reply["recent_prompts"][0]["text"],
        json!("/model claude-sonnet-5")
    );
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_subagent_tokens_count_toward_spend_but_not_context
#[test]
fn port_session_stats_subagent_only_responses_add_no_model_calls() {
    let reply = scenario(
        "claude-subagent-tokens-count-toward-spend-but-not-context",
        json!({ "agent": "claude", "responses": [["msg_b", 500, true]] }),
    );
    assert_eq!(reply["tokens"]["output"], json!(500));
    assert_eq!(reply["model_calls"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_synthetic_model_counts_an_api_error_without_tokens
#[test]
fn port_session_stats_real_model_error_text_is_not_counted_as_an_api_error() {
    let reply = scenario(
        "claude-synthetic-model-counts-an-api-error-without-tokens",
        json!({ "agent": "claude", "model": "claude-sonnet-5", "text": "API Error" }),
    );
    assert_eq!(reply["api_errors"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::claude_transcript_yields_work_marks_and_progress_runs
#[test]
fn port_session_stats_transcript_without_monitor_yields_no_progress_spans() {
    let reply = scenario(
        "claude-transcript-yields-work-marks-and-progress-runs",
        json!({ "agent": "claude", "fixture": "build-it transcript without any progress monitor" }),
    );
    assert_eq!(reply["progress_spans"].as_array().map(Vec::len), Some(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_falls_back_to_event_prompts_when_no_response_prompts_exist
#[test]
fn port_session_stats_response_prompts_win_over_event_prompts_when_both_exist() {
    let reply = scenario(
        "codex-falls-back-to-event-prompts-when-no-response-prompts-exist",
        json!({ "agent": "codex", "event_prompts": ["only event"], "response_prompts": ["a response"] }),
    );
    assert_eq!(reply["recent_prompts"][0]["text"], json!("a response"));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_quota_rises_become_per_minute_consumption_and_resets_only_rebaseline
#[test]
fn port_session_stats_reset_only_quota_change_records_no_consumption() {
    let reply = scenario(
        "codex-quota-rises-become-per-minute-consumption-and-resets-only-rebaseline",
        json!({ "agent": "codex", "fixture": "primary reset only" }),
    );
    assert_eq!(reply["primary_buckets"], json!([]));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_tracks_tools_turns_and_prefers_response_item_prompts
#[test]
fn port_session_stats_tool_outputs_alone_are_not_tool_calls() {
    let reply = scenario(
        "codex-tracks-tools-turns-and-prefers-response-item-prompts",
        json!({ "agent": "codex", "calls": [], "outputs": ["function_call_output"] }),
    );
    assert_eq!(reply["tool_calls"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::codex_uses_cumulative_totals_and_ignores_repeated_events
#[test]
fn port_session_stats_single_cumulative_token_event_counts_one_model_call() {
    let reply = scenario(
        "codex-uses-cumulative-totals-and-ignores-repeated-events",
        json!({ "agent": "codex", "fixture": "one cumulative token_count event" }),
    );
    assert_eq!(reply["model_calls"], json!(1));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::decimation_preserves_totals_and_peaks
#[test]
fn port_session_stats_sample_list_under_the_cap_is_not_decimated() {
    let reply = scenario(
        "decimation-preserves-totals-and-peaks",
        json!({ "samples": "fewer samples than SAMPLE_CAP" }),
    );
    assert_eq!(reply["decimated"], json!(false));
    assert_eq!(reply["merged_output_total"], reply["expected_output_total"]);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::identical_consecutive_prompts_collapse_into_one_record_with_a_repeat_count
#[test]
fn port_session_stats_non_consecutive_repeats_are_not_collapsed() {
    let reply = scenario(
        "identical-consecutive-prompts-collapse-into-one-record-with-a-repeat-count",
        json!({ "agent": "codex", "prompts": ["keep going", "stop", "keep going"] }),
    );
    assert_eq!(reply["recent_prompts"].as_array().map(Vec::len), Some(3));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::ingest_file_resumes_from_its_offset_and_skips_a_partial_last_line
#[test]
fn port_session_stats_complete_lines_in_one_step_are_all_counted() {
    let reply = scenario(
        "ingest-file-resumes-from-its-offset-and-skips-a-partial-last-line",
        json!({ "agent": "claude", "steps": ["complete msg_1 and msg_2:60"] }),
    );
    assert_eq!(reply["output_after_first"], json!(100));
    assert_eq!(reply["model_calls"], json!(2));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::main_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines
#[test]
fn port_session_stats_main_transcript_passes_lose_no_lines() {
    let reply = scenario(
        "main-transcript-reads-are-bounded-per-pass-and-resume-without-losing-lines",
        json!({ "file": "two passes of lines", "kind": "main" }),
    );
    assert_eq!(reply["lines_lost"], json!(0));
    assert_eq!(reply["lines_seen"], reply["lines_expected"]);
}

/// REQ: PORT:ilium-client/src/session_stats.rs::malformed_and_irrelevant_lines_are_ignored
#[test]
fn port_session_stats_malformed_lines_add_no_timeline_events() {
    let reply = scenario(
        "malformed-and-irrelevant-lines-are-ignored",
        json!({ "agent": "claude", "lines": ["truncated json"], "timeline": "four buckets" }),
    );
    assert_eq!(reply["buckets_event_total"], json!(0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::oversized_complete_line_is_explicitly_incomplete_and_does_not_advance_offset
#[test]
fn port_session_stats_normal_sized_line_is_read_and_advances_offset() {
    let reply = scenario(
        "oversized-complete-line-is-explicitly-incomplete-and-does-not-advance-offset",
        json!({ "agent": "claude", "oversized_line": false }),
    );
    assert_eq!(reply["error"], Value::Null);
    assert!(reply["offset"].as_u64().is_some_and(|offset| offset > 0));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::progress_outcomes_are_read_from_delivered_monitor_text
#[test]
fn port_session_stats_failure_text_of_another_monitor_number_is_still_a_failure() {
    let reply = scenario(
        "progress-outcomes-are-read-from-delivered-monitor-text",
        json!({ "texts": ["Ilium progress monitor 9 reports that job failed"] }),
    );
    assert_eq!(reply["outcomes"][0], json!("failure"));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::real_transcript_report
#[test]
fn port_session_stats_missing_sample_path_is_reported_as_an_error() {
    let reply = scenario(
        "real-transcript-report",
        json!({ "sample_env": "ILIUM_STATS_SAMPLE_UNSET" }),
    );
    assert!(reply["error"].is_string(), "{reply}");
}

/// REQ: PORT:ilium-client/src/session_stats.rs::sidecar_transcript_reads_are_bounded_per_pass_and_resume_without_losing_lines
#[test]
fn port_session_stats_sidecar_offset_never_moves_backwards_between_passes() {
    let reply = scenario(
        "sidecar-transcript-reads-are-bounded-per-pass-and-resume-without-losing-lines",
        json!({ "file": "two passes of lines", "kind": "sidecar" }),
    );
    assert!(reply["offset_after_second"]
        .as_u64()
        .is_some_and(|second| reply["offset_after_first"]
            .as_u64()
            .is_some_and(|first| second >= first)));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::timeline_buckets_fold_samples_and_activity
#[test]
fn port_session_stats_two_events_in_one_bucket_fold_into_one_total() {
    let reply = scenario(
        "timeline-buckets-fold-samples-and-activity",
        json!({ "samples": "two events in one bucket" }),
    );
    assert_eq!(reply["buckets"].as_array().map(Vec::len), Some(4));
    assert_eq!(reply["buckets_event_total"], json!(2));
}

/// REQ: PORT:ilium-client/src/session_stats.rs::tool_result_lines_are_never_parsed_but_failures_are_counted
#[test]
fn port_session_stats_successful_tool_result_is_not_counted_as_a_failure() {
    let reply = scenario(
        "tool-result-lines-are-never-parsed-but-failures-are-counted",
        json!({ "agent": "claude", "tool_result_bytes": 100000, "is_error": false }),
    );
    assert_eq!(reply["tool_errors"], json!(0));
    assert_eq!(reply["records_parsed"], json!(0));
}
