//! Parser rules against format-faithful synthetic transcripts
//! (`tests/fixtures/*.jsonl`; no real prompts, paths or identifiers).

use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};
use ilium_compaction_analysis::trace::{
    CompactionTrigger, SessionTrace, FLAG_CACHE_COLD, FLAG_COMPACTION_REQUEST,
    FLAG_FIRST_AFTER_COMPACTION, FLAG_GAP_COLD,
};
use ilium_compaction_analysis::AgentKind;

fn parse(agent: AgentKind, text: &str, subagent: bool) -> SessionTrace {
    let mut builder = TraceBuilder::new(agent).with_subagent(subagent);
    for line in text.split('\n') {
        // The documented scan loop: prefilter first, then feed.
        if line_may_matter(agent, line.as_bytes()) {
            builder.feed_line(line.as_bytes());
        }
    }
    builder.finish()
}

fn claude_fixture() -> SessionTrace {
    parse(
        AgentKind::ClaudeCode,
        include_str!("fixtures/claude_session.jsonl"),
        false,
    )
}

fn codex_fixture() -> SessionTrace {
    parse(
        AgentKind::Codex,
        include_str!("fixtures/codex_session.jsonl"),
        false,
    )
}

#[test]
fn claude_dedupes_streamed_message_lines_keeping_the_final_usage() {
    let trace = claude_fixture();
    assert_eq!(trace.turns.len(), 9);
    let first = trace.turns[0];
    assert_eq!(first.context_tokens, 30_003);
    assert_eq!(first.input_tokens, 3);
    assert_eq!(first.cache_write_1h_tokens, 30_000);
    assert_eq!(first.cache_write_5m_tokens, 0);
    // The partial streaming line said 50 output tokens; the final line 200.
    assert_eq!(first.output_tokens, 200);
    assert_eq!(trace.counters.duplicate_requests, 3);
}

#[test]
fn claude_advisor_iterations_do_not_double_count_the_context() {
    let trace = claude_fixture();
    let advisor_turn = trace.turns[2];
    // Context of the first main iteration only: 2 + 31_500 + 2_000.
    assert_eq!(advisor_turn.context_tokens, 33_502);
    // Cost counts both main iterations but never the advisor model's tokens.
    assert_eq!(advisor_turn.input_tokens, 4);
    assert_eq!(advisor_turn.cache_read_tokens, 65_000);
    assert_eq!(advisor_turn.cache_write_1h_tokens, 2_500);
    assert_eq!(advisor_turn.output_tokens, 500);
}

#[test]
fn claude_skips_housekeeping_synthetic_and_sidechain_requests_in_main_sessions() {
    let trace = claude_fixture();
    assert_eq!(trace.models, vec!["claude-sonnet-5-5".to_string()]);
    assert_eq!(trace.counters.skipped_model_requests, 1);
    assert_eq!(trace.counters.sidechain_requests_skipped, 1);

    let subagent = parse(
        AgentKind::ClaudeCode,
        include_str!("fixtures/claude_session.jsonl"),
        true,
    );
    assert!(subagent.is_subagent);
    assert_eq!(subagent.counters.skipped_model_requests, 0);
    assert_eq!(subagent.counters.sidechain_requests_skipped, 0);
    // The haiku and sidechain requests are kept; `<synthetic>` never is.
    assert_eq!(subagent.turns.len(), 11);
    assert_eq!(subagent.models.len(), 2);
}

#[test]
fn claude_flags_cold_cache_and_idle_gaps() {
    let trace = claude_fixture();
    let warm = trace.turns[3];
    assert_eq!(warm.flags & (FLAG_CACHE_COLD | FLAG_GAP_COLD), 0);
    assert_eq!(warm.gap_seconds(), 270);
    let cold = trace.turns[4];
    assert!(cold.has_flag(FLAG_CACHE_COLD));
    assert!(cold.has_flag(FLAG_GAP_COLD));
    assert_eq!(cold.gap_seconds(), 8_670);
    // Tier split and untiered writes.
    let mixed = trace.turns[6];
    assert_eq!(mixed.cache_write_5m_tokens, 10_000);
    assert_eq!(mixed.cache_write_1h_tokens, 29_997);
    assert_eq!(mixed.context_tokens, 100_000);
}

#[test]
fn claude_compact_boundary_uses_the_next_turn_not_the_logged_post_tokens() {
    let trace = claude_fixture();
    assert_eq!(trace.compactions.len(), 2);
    let auto = &trace.compactions[0];
    assert_eq!(auto.trigger, CompactionTrigger::Auto);
    assert_eq!(auto.pre_tokens, 100_000);
    assert_eq!(auto.last_pre_context_tokens, 100_000);
    assert_eq!(auto.logged_post_tokens, 17_000);
    assert_eq!(auto.turn_index, 7);
    assert!(auto.is_measured());
    // Measured post-compaction request: the first following assistant turn.
    assert_eq!(auto.post_context_tokens, 40_000);
    assert_eq!(auto.post_cache_read_tokens, 12_000);
    assert_eq!(auto.post_cache_write_1h_tokens, 27_997);
    assert_eq!(auto.post_input_tokens, 3);
    assert_eq!(auto.post_fresh_tokens(), 28_000);
    // 7000 summary characters at 3.5 characters per token.
    assert_eq!(auto.summary_tokens, 2_000);
    assert!(auto.summary_estimated);
    assert_eq!(auto.duration_ms, 5_000);
    let first_after = trace.turns[7];
    assert!(first_after.has_flag(FLAG_FIRST_AFTER_COMPACTION));
    assert!(!first_after.has_flag(FLAG_CACHE_COLD));

    let manual = &trace.compactions[1];
    assert_eq!(manual.trigger, CompactionTrigger::Manual);
    assert_eq!(manual.pre_tokens, 41_000);
    assert!(!manual.is_measured());
    assert_eq!(manual.turn_index as usize, trace.turns.len());
}

#[test]
fn claude_tolerates_non_json_and_truncated_tail_lines() {
    // Feed every line (no caller-side prefilter) so stray text is seen too.
    let mut builder = TraceBuilder::new(AgentKind::ClaudeCode);
    for line in include_str!("fixtures/claude_session.jsonl").split('\n') {
        builder.feed_line(line.as_bytes());
    }
    let trace = builder.finish();
    assert_eq!(trace.counters.lines_not_json, 1);
    // The cut-off last line mentions "usage", so it reaches the parser and
    // fails there instead of aborting the scan.
    assert_eq!(trace.counters.lines_unparseable, 1);
}

#[test]
fn oversize_lines_are_skipped_and_counted() {
    let text = include_str!("fixtures/claude_session.jsonl");
    let mut builder = TraceBuilder::new(AgentKind::ClaudeCode).with_max_line_bytes(7_000);
    for line in text.split('\n') {
        builder.feed_line(line.as_bytes());
    }
    let trace = builder.finish();
    // Only the 7000-character summary line exceeds the limit.
    assert_eq!(trace.counters.lines_skipped_oversize, 1);
    assert_eq!(trace.compactions[0].summary_tokens, 0);
    assert_eq!(trace.turns.len(), 9);
}

#[test]
fn codex_dedupes_by_response_id_and_splits_cached_input() {
    let trace = codex_fixture();
    assert_eq!(trace.turns.len(), 8);
    assert_eq!(trace.counters.duplicate_requests, 1);
    let second = trace.turns[1];
    assert_eq!(second.context_tokens, 30_400);
    assert_eq!(second.cache_read_tokens, 29_000);
    assert_eq!(second.input_tokens, 1_400);
    assert_eq!(
        second.cache_write_5m_tokens + second.cache_write_1h_tokens,
        0
    );
    assert_eq!(second.output_tokens, 150);
}

#[test]
fn codex_tracks_the_model_and_the_context_window() {
    let trace = codex_fixture();
    assert_eq!(
        trace.models,
        vec!["gpt-6.1-sol".to_string(), "gpt-6-luna".to_string()]
    );
    assert_eq!(trace.turns[3].model, 0);
    assert_eq!(trace.turns[4].model, 1);
    assert_eq!(trace.context_window_tokens, Some(258_400));
}

#[test]
fn codex_compaction_links_request_pre_and_post() {
    let trace = codex_fixture();
    // The second `compacted` line carries no response id and is unmatched.
    assert_eq!(trace.compactions.len(), 1);
    assert_eq!(trace.counters.compactions_unmatched, 1);
    let event = &trace.compactions[0];
    assert_eq!(event.trigger, CompactionTrigger::Unknown);
    // Trigger quantity: last work response input + output.
    assert_eq!(event.pre_tokens, 229_700);
    assert_eq!(event.last_pre_context_tokens, 229_000);
    assert_eq!(event.summary_tokens, 4_200);
    assert!(!event.summary_estimated);
    assert_eq!(event.request_turn_index, Some(5));
    assert!(trace.turns[5].has_flag(FLAG_COMPACTION_REQUEST));
    assert_eq!(event.turn_index, 6);
    assert_eq!(event.post_context_tokens, 50_000);
    assert_eq!(event.post_cache_read_tokens, 12_800);
    assert_eq!(event.post_input_tokens, 37_200);
    assert!(event.replacement_history_tokens > 900);
    assert!(trace.turns[6].has_flag(FLAG_FIRST_AFTER_COMPACTION));
    assert!(!trace.turns[6].has_flag(FLAG_CACHE_COLD));
    assert_eq!(trace.counters.lines_unparseable, 1);
}

#[test]
fn codex_subagent_and_window_come_from_session_meta() {
    let trace = parse(
        AgentKind::Codex,
        include_str!("fixtures/codex_subagent.jsonl"),
        false,
    );
    assert!(trace.is_subagent);
    assert_eq!(trace.context_window_tokens, Some(272_000));
    assert_eq!(trace.turns.len(), 1);
}

#[test]
fn parsed_traces_round_trip_through_the_cache_format() {
    for trace in [claude_fixture(), codex_fixture()] {
        let bytes = trace.to_json_bytes().unwrap();
        let back = SessionTrace::from_json_bytes(&bytes).unwrap();
        assert_eq!(back, trace);
    }
}

// ---- tool features (rework inputs) ----

use ilium_compaction_analysis::trace::{
    ToolFeatures, HASH_KIND_MASK, HASH_KIND_PATH, HASH_KIND_SEARCH, TOOL_FLAG_EDIT,
    TOOL_FLAG_WRITE_HEURISTIC,
};

fn reads_of(tools: &ToolFeatures, turn: usize) -> Vec<u32> {
    let offsets = tools.offsets();
    tools.read_hashes[offsets.read[turn]..offsets.read[turn + 1]].to_vec()
}

#[test]
fn claude_tool_features_follow_the_message_across_its_lines() {
    let trace = claude_fixture();
    let tools = &trace.tools;
    assert!(tools.is_consistent_with(trace.turns.len()));
    // a1: a Grep tool use on the second (streamed) line of the message.
    let grep = tools.turns[0];
    assert_eq!(
        (grep.tool_calls, grep.read_calls, grep.command_calls),
        (1, 1, 0)
    );
    assert_eq!(reads_of(tools, 0)[0] & HASH_KIND_MASK, HASH_KIND_SEARCH);
    // a2: Read of an absolute path.
    let read = tools.turns[1];
    assert_eq!((read.tool_calls, read.read_calls), (1, 1));
    let plan_hash = reads_of(tools, 1)[0];
    assert_eq!(plan_hash & HASH_KIND_MASK, HASH_KIND_PATH);
    // a3: `cat plan.md && rg ...` is ONE read-like command; the relative path
    // resolves against the logged cwd to the same hash as the Read above.
    let command = tools.turns[2];
    assert_eq!(
        (
            command.tool_calls,
            command.read_calls,
            command.command_calls
        ),
        (1, 1, 1)
    );
    let hashes = reads_of(tools, 2);
    assert_eq!(hashes[0], plan_hash);
    assert_eq!(hashes[1] & HASH_KIND_MASK, HASH_KIND_SEARCH);
    assert_eq!(tools.offsets().command[3] - tools.offsets().command[2], 1);
    // a4: three lines, the last repeating a tool-use id: counted once each.
    let split = tools.turns[3];
    assert_eq!(
        (split.tool_calls, split.read_calls, split.command_calls),
        (2, 1, 1)
    );
    assert_eq!(reads_of(tools, 3), vec![plan_hash]);
    // a5: dedicated edit tool; a6: shell write heuristic (flagged separately).
    assert_eq!(tools.turns[4].flags, TOOL_FLAG_EDIT);
    assert!(tools.turns[4].has_edit());
    assert_eq!(tools.turns[5].flags, TOOL_FLAG_WRITE_HEURISTIC);
    assert!(!tools.turns[5].has_edit() && tools.turns[5].has_any_write());
    // Turns without tool calls have empty rows.
    assert_eq!(tools.turns[8].tool_calls, 0);
    assert_eq!(trace.counters.tool_calls_dropped, 0);
}

#[test]
fn codex_tool_calls_attach_to_the_next_usage_record() {
    let trace = codex_fixture();
    let tools = &trace.tools;
    assert!(tools.is_consistent_with(trace.turns.len()));
    // Calls before resp_002: exec code (cat+rg, one read-like command), a
    // function-call shell command, an apply_patch inside code, a non-shell
    // tool; the custom_tool_call_output is not a call.
    let second = tools.turns[1];
    assert_eq!(second.tool_calls, 4);
    assert_eq!(second.command_calls, 2);
    assert_eq!(second.read_calls, 1);
    assert!(second.has_edit());
    // cat -> path hash, rg -> whole-command search hash.
    let hashes = reads_of(tools, 1);
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes[0] & HASH_KIND_MASK, HASH_KIND_PATH);
    assert_eq!(hashes[1] & HASH_KIND_MASK, HASH_KIND_SEARCH);
    // resp_003: `sed -n '1,40p' src/main.rs` re-reads the same file.
    let third = tools.turns[2];
    assert_eq!(
        (third.tool_calls, third.read_calls, third.command_calls),
        (1, 1, 1)
    );
    assert_eq!(reads_of(tools, 2), vec![hashes[0]]);
    // The first request has no calls.
    assert_eq!(tools.turns[0].tool_calls, 0);
}

#[test]
fn serialized_traces_hold_hashes_only_never_paths_or_commands() {
    for trace in [claude_fixture(), codex_fixture()] {
        let json = String::from_utf8(trace.to_json_bytes().unwrap()).unwrap();
        for secret in [
            "secret-user",
            "topsecret",
            "TOPSECRETTOKEN",
            "plan.md",
            "main.rs",
            "lib.rs",
            "cargo",
            "rg -n",
            "src/",
        ] {
            assert!(!json.contains(secret), "{secret} leaked into the cache");
        }
    }
}

#[test]
fn version_one_caches_are_refused_so_the_caller_rescans() {
    use ilium_compaction_analysis::{AnalysisError, TRACE_FORMAT_VERSION};
    assert_eq!(TRACE_FORMAT_VERSION, 2);
    let mut value: serde_json::Value =
        serde_json::from_slice(&claude_fixture().to_json_bytes().unwrap()).unwrap();
    value["format_version"] = serde_json::json!(1);
    value.as_object_mut().unwrap().remove("tools");
    let old = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        SessionTrace::from_json_bytes(&old),
        Err(AnalysisError::TraceVersionMismatch {
            found: 1,
            expected: 2
        })
    ));
}

#[test]
fn cross_file_dedupe_keeps_tool_features_aligned() {
    use ilium_compaction_analysis::dedupe::dedupe_across_traces;
    // An "earlier file" holding only the first five requests of the session,
    // and the full session as a later (resumed) file.
    let full = claude_fixture();
    let mut earlier = full.clone();
    let keep: Vec<bool> = (0..full.turns.len()).map(|index| index < 5).collect();
    earlier.turns.truncate(5);
    earlier.turn_id_hashes.truncate(5);
    earlier.tools.retain_turns(&keep);
    earlier.compactions.clear();
    let mut traces = vec![earlier, full.clone()];
    let report = dedupe_across_traces(&mut traces);
    assert_eq!(report.turns_removed, 5);
    let later = &traces[1];
    assert_eq!(later.turns.len(), full.turns.len() - 5);
    assert!(later.tools.is_consistent_with(later.turns.len()));
    // The surviving rows are the original rows of the later requests.
    assert_eq!(later.tools.turns[..], full.tools.turns[5..]);
    assert_eq!(later.tools.turns[0].flags, TOOL_FLAG_WRITE_HEURISTIC);
}
