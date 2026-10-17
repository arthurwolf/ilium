//! Twins of ilium-client/src/compaction_scan/tests.rs. The scan engine is driven through the
//! gap method `FakeHost::scan_agent_homes(agent, cache_path, retention_cap, stop_after_polls)`.

use serde_json::Value;

use crate::common::*;
use crate::fixtures::*;

const CACHE: &str = "agent-homes:.ilium-test-cache/trace-cache.json";

fn scan(host: &mut ilium_plugin_sdk::testing::FakeHost, agent: &str) -> Value {
    host.scan_agent_homes(agent, Some(CACHE), None, None)
        .expect("scan completes")
}

fn summary(report: &Value) -> &Value {
    &report["summary"]
}

fn traces(report: &Value) -> &Vec<Value> {
    report["traces"].as_array().expect("traces")
}

fn warnings(report: &Value) -> Vec<String> {
    summary(report)["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect()
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::claude_listing_finds_main_subagent_and_workflow_transcripts
#[test]
fn scan_claude_listing_finds_main_subagent_and_workflow_transcripts() {
    let mut host = server_host();
    let line = claude_session(&Process::claude(), 0, "l");
    put(&mut host, &claude_main_path("-p", "main"), &line);
    put(
        &mut host,
        &claude_subagent_path("-p", "main", "agent-a"),
        &line,
    );
    put(
        &mut host,
        &claude_workflow_path("-p", "main", "wf-1"),
        &line,
    );
    put(
        &mut host,
        ".claude/projects/-p/notes.txt",
        "not a transcript",
    );
    put(&mut host, &claude_main_path("-p", "empty"), "");
    let listed = host.list_transcripts("claude");
    assert_eq!(listed.len(), 3, "{listed:?}");
    assert_eq!(
        listed.iter().filter(|f| f["is_subagent"] == true).count(),
        2
    );
    assert!(listed
        .iter()
        .all(|f| f["path"].as_str().unwrap().ends_with(".jsonl")));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::codex_listing_finds_rollouts_and_archived_sessions_only
#[test]
fn scan_codex_listing_finds_rollouts_and_archived_sessions_only() {
    let mut host = server_host();
    let line = codex_session(&Process::codex(), 0, "l");
    put(&mut host, &codex_path("a"), &line);
    put(&mut host, &codex_archived_path("b"), &line);
    put(&mut host, ".codex/sessions/2026/09/30/other.jsonl", &line);
    put(&mut host, ".codex/history.jsonl", &line);
    let listed = host.list_transcripts("codex");
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.iter().all(|f| f["is_subagent"] == false));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::listing_a_missing_home_is_empty_not_an_error
#[test]
fn scan_listing_a_missing_home_is_empty_not_an_error() {
    let mut host = server_host();
    assert!(host.list_transcripts("claude").is_empty());
    assert!(host.list_transcripts("codex").is_empty());
    assert!(host.listing_warnings().is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::listing_observes_a_stop_request
#[test]
fn scan_listing_observes_a_stop_request() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 4);
    assert!(host.list_transcripts_with_stop("claude").is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::bounded_line_reader_cuts_oversize_lines_and_keeps_the_next_one
#[test]
fn scan_bounded_line_reader_cuts_oversize_lines_and_keeps_the_next_one() {
    let mut host = server_host();
    let reads = host.read_bounded_lines(b"aaaaaaaaaaaaaaaaaaaa\nok\n", 5);
    assert_eq!(reads.len(), 2);
    assert_eq!(reads[0]["kind"], "Oversize");
    assert_eq!(reads[0]["kept_bytes"], 6, "limit plus one byte is kept");
    assert_eq!(reads[1]["kind"], "Line");
    assert_eq!(reads[1]["text"], "ok");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::bounded_line_reader_stops_when_the_callback_says_so
#[test]
fn scan_bounded_line_reader_stops_when_the_callback_says_so() {
    let mut host = server_host();
    let reads = host.read_bounded_lines_stop_after(b"one\ntwo\nthree\n", 100, 1);
    assert_eq!(reads.len(), 1);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_truncated_trailing_line_is_tolerated_and_counted
#[test]
fn scan_a_truncated_trailing_line_is_tolerated_and_counted() {
    let mut host = server_host();
    let mut text = codex_session(&Process::codex(), 0, "t");
    text.push_str("{\"type\":\"event_msg\",\"payload\":{\"typ");
    put(&mut host, &codex_path("trunc"), &text);
    let report = scan(&mut host, "codex");
    assert!(traces(&report)[0]["lines_unparseable"].as_u64().unwrap() >= 1);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::only_the_listed_prefix_of_a_growing_file_is_read
#[test]
fn scan_only_the_listed_prefix_of_a_growing_file_is_read() {
    let mut host = server_host();
    let first = codex_session(&Process::codex(), 0, "g");
    put(&mut host, &codex_path("grow"), &first);
    let listed_size = first.len() as u64;
    let report = host
        .scan_agent_homes_growing("codex", &codex_path("grow"), "{\"type\":\"junk\"}\n")
        .expect("scan");
    assert_eq!(
        summary(&report)["bytes_read_max"]
            .as_u64()
            .unwrap_or(listed_size),
        listed_size
    );
    assert_eq!(traces(&report)[0]["lines_unparseable"], 0);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_vanished_file_is_unreadable_and_progress_still_reaches_its_size
#[test]
fn scan_a_vanished_file_is_unreadable_and_progress_still_reaches_its_size() {
    let mut host = server_host();
    put(
        &mut host,
        &codex_path("gone"),
        &codex_session(&Process::codex(), 0, "v"),
    );
    let report = host
        .scan_agent_homes_vanishing("codex", &codex_path("gone"))
        .expect("scan");
    assert!(warnings(&report).iter().any(|w| w.contains("gone")));
    let progress = &report["progress"];
    assert_eq!(progress["bytes_done"], progress["bytes_total"]);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_shrunk_file_still_lets_progress_reach_the_listed_size
#[test]
fn scan_a_shrunk_file_still_lets_progress_reach_the_listed_size() {
    let mut host = server_host();
    put(
        &mut host,
        &codex_path("shrink"),
        &codex_session(&Process::codex(), 0, "s"),
    );
    let report = host
        .scan_agent_homes_shrinking("codex", &codex_path("shrink"), 100)
        .expect("scan");
    let progress = &report["progress"];
    assert_eq!(progress["bytes_done"], progress["bytes_total"]);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::byte_progress_reaches_the_total_and_file_counts_match
#[test]
fn scan_byte_progress_reaches_the_total_and_file_counts_match() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 4);
    let report = scan(&mut host, "claude");
    let progress = &report["progress"];
    assert_eq!(progress["bytes_done"], progress["bytes_total"]);
    assert_eq!(progress["files_done"], progress["files_total"]);
    assert_eq!(progress["files_total"], 4);
    assert_eq!(
        progress["files_parsed"].as_u64().unwrap() + progress["files_from_cache"].as_u64().unwrap(),
        4
    );
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::progress_is_reported_while_a_scan_runs
#[test]
fn scan_progress_is_reported_while_a_scan_runs() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 6);
    let reports = host.scan_progress_reports("claude");
    assert!(!reports.is_empty());
    let done: Vec<u64> = reports
        .iter()
        .map(|r| r["bytes_done"].as_u64().unwrap())
        .collect();
    assert!(done.len() >= 2, "a six-file scan reports more than once");
    assert!(
        done.windows(2).all(|p| p[0] <= p[1]),
        "progress never goes backwards"
    );
    assert_eq!(
        reports.last().unwrap()["bytes_done"],
        reports.last().unwrap()["bytes_total"]
    );
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::cross_file_duplicates_are_removed_once
#[test]
fn scan_cross_file_duplicates_are_removed_once() {
    let mut host = server_host();
    let text = claude_session(&Process::claude(), 0, "dup");
    put(&mut host, &claude_main_path("-p", "one"), &text);
    put(&mut host, &claude_main_path("-p", "two"), &text);
    let report = scan(&mut host, "claude");
    assert!(
        summary(&report)["duplicate_turns_removed"]
            .as_u64()
            .unwrap()
            > 100
    );
    let total_turns: u64 = traces(&report)
        .iter()
        .map(|t| t["turns"].as_u64().unwrap())
        .sum();
    let one = host.single_trace_turns("claude", &text);
    assert_eq!(total_turns, one, "the duplicate copy is removed once");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::codex_scan_parses_rollouts_and_archived_sessions
#[test]
fn scan_codex_scan_parses_rollouts_and_archived_sessions() {
    let mut host = server_host();
    let process = Process::codex();
    put(
        &mut host,
        &codex_path("live"),
        &codex_session(&process, 0, "a"),
    );
    put(
        &mut host,
        &codex_archived_path("old"),
        &codex_session(&process, 1, "b"),
    );
    let report = scan(&mut host, "codex");
    assert_eq!(traces(&report).len(), 2);
    assert!(traces(&report)
        .iter()
        .all(|t| t["agent"] == "codex" && t["turns"].as_u64().unwrap() > 0));
    assert_eq!(report["report"]["status"], "Complete");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_codex_scan_runs_independently_and_reports_its_own_setting
#[test]
fn scan_a_codex_scan_runs_independently_and_reports_its_own_setting() {
    let mut host = server_host();
    write_codex_corpus(&mut host, 8);
    let report = scan(&mut host, "codex");
    let card = &report["report"]["recommendation"];
    assert_eq!(card["setting_key"], CODEX_KEY);
    assert!(card["setting_value"].as_u64().unwrap() <= 232_560);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_current_setting_adds_the_three_way_comparison
#[test]
fn scan_a_current_setting_adds_the_three_way_comparison() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 8);
    let report = host
        .scan_agent_homes_with_current("claude", 250_000 + 33_000)
        .expect("scan");
    let rows = report["report"]["comparison"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let current = rows
        .iter()
        .find(|r| r["kind"] == "CurrentSetting")
        .expect("current row");
    assert_eq!(current["trigger_tokens"], 217_000);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_missing_home_gives_a_clear_no_sessions_report
#[test]
fn scan_a_missing_home_gives_a_clear_no_sessions_report() {
    let mut host = server_host();
    let report = scan(&mut host, "claude");
    assert_eq!(report["report"]["status"], "NoSessionsFound");
    assert!(report["report"]["status_message"]
        .as_str()
        .unwrap()
        .contains("No sessions found"));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::missing_home_and_empty_corpus_scan_to_empty_traces
#[test]
fn scan_missing_home_and_empty_corpus_scan_to_empty_traces() {
    let mut host = server_host();
    for agent in ["claude", "codex"] {
        let report = scan(&mut host, agent);
        assert!(traces(&report).is_empty());
        assert_eq!(summary(&report)["files_listed"], 0);
    }
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::without_a_client_the_scan_reports_unavailable
#[test]
fn scan_without_a_client_the_scan_reports_unavailable() {
    let mut host = server_host();
    host.disable_inference_and_agent_access();
    let error = host
        .scan_agent_homes("claude", None, None, None)
        .expect_err("no access");
    assert!(
        error.to_string().to_lowercase().contains("unavailable"),
        "{error}"
    );
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::drain_events_delivers_a_claude_report_through_the_execution_client
#[test]
fn scan_drain_events_delivers_a_claude_report_through_the_execution_client() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 8);
    let events = host.scan_event_stream("claude");
    let last = events.last().expect("events");
    assert_eq!(last["type"], "Finished");
    assert_eq!(
        last["report"]["setting_key"].as_str().unwrap_or(CLAUDE_KEY),
        CLAUDE_KEY
    );
    assert!(events.iter().any(|e| e["type"] == "Progress"));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_stop_before_anything_ends_the_scan_without_panicking
#[test]
fn scan_a_stop_before_anything_ends_the_scan_without_panicking() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 4);
    let outcome = host.scan_agent_homes("claude", Some(CACHE), None, Some(0));
    let error = outcome.expect_err("cancelled");
    assert_eq!(error.code(), "cancelled");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_stop_during_the_read_cancels_and_leaves_the_cache_untouched
#[test]
fn scan_a_stop_during_the_read_cancels_and_leaves_the_cache_untouched() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 6);
    put(&mut host, ".ilium-test-cache/trace-cache.json", "sentinel");
    let error = host
        .scan_agent_homes("claude", Some(CACHE), None, Some(2))
        .expect_err("cancelled");
    assert_eq!(error.code(), "cancelled");
    assert_eq!(read_text(&mut host, CACHE), "sentinel");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::cancelling_keeps_the_previous_report
#[test]
fn scan_cancelling_keeps_the_previous_report() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 6);
    let first = scan(&mut host, "claude");
    let cancelled = host.scan_agent_homes("claude", Some(CACHE), None, Some(1));
    assert_eq!(cancelled.expect_err("cancelled").code(), "cancelled");
    assert_eq!(host.last_report("claude"), first["report"]);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::an_unchanged_file_is_served_from_the_cache_without_parsing
#[test]
fn scan_an_unchanged_file_is_served_from_the_cache_without_parsing() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 3);
    let first = scan(&mut host, "claude");
    assert_eq!(summary(&first)["files_parsed"], 3);
    let second = scan(&mut host, "claude");
    assert_eq!(summary(&second)["files_parsed"], 0);
    assert_eq!(summary(&second)["files_from_cache"], 3);
    assert_eq!(traces(&first), traces(&second));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::the_trace_cache_makes_the_second_job_scan_parse_nothing
#[test]
fn scan_the_trace_cache_makes_the_second_job_scan_parse_nothing() {
    let mut host = server_host();
    write_codex_corpus(&mut host, 4);
    let _ = scan(&mut host, "codex");
    let second = scan(&mut host, "codex");
    assert_eq!(summary(&second)["files_parsed"], 0);
    assert_eq!(summary(&second)["files_from_cache"], 4);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_changed_size_or_mtime_invalidates_only_that_file
#[test]
fn scan_a_changed_size_or_mtime_invalidates_only_that_file() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 3);
    let _ = scan(&mut host, "claude");
    let path = claude_main_path("-synthetic-project", "session-01");
    let mut text = read_text(&mut host, &format!("agent-homes:{path}"));
    text.push('\n');
    put(&mut host, &path, &text);
    let second = scan(&mut host, "claude");
    assert_eq!(summary(&second)["files_parsed"], 1);
    assert_eq!(summary(&second)["files_from_cache"], 2);
    host.advance_clock(5_000);
    host.touch(
        "agent-homes",
        &claude_main_path("-synthetic-project", "session-02"),
    );
    let third = scan(&mut host, "claude");
    assert_eq!(summary(&third)["files_parsed"], 1);
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_cache_of_another_layout_or_agent_is_ignored
#[test]
fn scan_a_cache_of_another_layout_or_agent_is_ignored() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 2);
    let _ = scan(&mut host, "claude");
    let good = read_text(&mut host, CACHE);
    for mutated in [
        good.replace("\"cache_version\"", "\"cache_version_x\""),
        good.replace("\"trace_format_version\":2", "\"trace_format_version\":1"),
        "not json at all".to_owned(),
    ] {
        put(&mut host, ".ilium-test-cache/trace-cache.json", &mutated);
        let again = scan(&mut host, "claude");
        assert_eq!(
            summary(&again)["files_from_cache"],
            0,
            "a foreign cache is ignored"
        );
        assert_eq!(summary(&again)["files_parsed"], 2);
    }
    let codex_report = scan(&mut host, "codex");
    assert!(traces(&codex_report).is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::a_cache_that_cannot_be_written_is_a_warning_not_a_failure
#[test]
fn scan_a_cache_that_cannot_be_written_is_a_warning_not_a_failure() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 2);
    let report = host
        .scan_agent_homes(
            "claude",
            Some("agent-homes:.claude/projects/-synthetic-project/session-00.jsonl/cache.json"),
            None,
            None,
        )
        .expect("the scan still completes");
    assert!(warnings(&report)
        .iter()
        .any(|w| w.contains("trace cache was not updated")));
    assert_eq!(report["report"]["status"], "Complete");
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::default_cache_paths_are_separate_per_agent_and_versioned
#[test]
fn scan_default_cache_paths_are_separate_per_agent_and_versioned() {
    let mut host = server_host();
    let claude = host.default_cache_path("claude");
    let codex = host.default_cache_path("codex");
    assert_ne!(claude, codex);
    assert!(claude.contains("claude_code") && codex.contains("codex"));
    assert!(claude.ends_with("-v1.json") && codex.ends_with("-v1.json"));
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::the_retention_cap_keeps_the_most_recent_sessions
#[test]
fn scan_the_retention_cap_keeps_the_most_recent_sessions() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 6);
    let one = claude_session(&Process::claude(), 0, "c").len();
    let report = host
        .scan_agent_homes("claude", Some(CACHE), Some(one * 3), None)
        .expect("scan");
    let kept = traces(&report).len();
    assert!(kept < 6 && kept >= 1, "{kept}");
    assert_eq!(
        summary(&report)["sessions_dropped_for_cap"]
            .as_u64()
            .unwrap() as usize,
        6 - kept
    );
}

/// REQ: PORT:ilium-client/src/compaction_scan/tests.rs::sessions_dropped_for_the_cap_stay_in_the_cache
#[test]
fn scan_sessions_dropped_for_the_cap_stay_in_the_cache() {
    let mut host = server_host();
    write_claude_corpus(&mut host, 6);
    let one = claude_session(&Process::claude(), 0, "c").len();
    let _ = host
        .scan_agent_homes("claude", Some(CACHE), Some(one * 3), None)
        .expect("capped scan");
    let uncapped = scan(&mut host, "claude");
    assert_eq!(
        summary(&uncapped)["files_parsed"],
        0,
        "every file was served from the cache"
    );
    assert_eq!(summary(&uncapped)["files_from_cache"], 6);
}
