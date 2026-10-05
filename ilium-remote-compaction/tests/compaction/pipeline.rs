//! Summarization pipeline behaviour seen through `compact_session`.

use std::sync::atomic::{AtomicBool, Ordering};

use ilium_remote_compaction::{
    latest_context_usage, transcript_is_at_pause_point, AgentKind, CompactionError,
    CompactionEvent, SummarizerError, Technique,
};

use crate::common::{
    dir_listing, good_reply, install_fixture, request_for, run, run_ok, small_options, Fake,
};

#[test]
fn single_chunk_runs_one_call_and_reports_steps_progress_and_tokens() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::always(&good_reply(Technique::ClaudeCode));
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    let (outcome, events) = run_ok(&request, &summarizer);

    assert_eq!(summarizer.calls(), 1);
    assert_eq!(outcome.chunks, 1);
    assert!(!outcome.used_fallback);
    assert!(outcome.summary_chars > 50);
    assert!(outcome.redactions >= 1, "the fixture holds a secret");
    assert_eq!(outcome.session_id, crate::common::CLAUDE_SESSION_ID);

    let steps: Vec<usize> = events
        .iter()
        .filter_map(|event| match event {
            CompactionEvent::Step { index, total, .. } => {
                assert_eq!(*total, 4);
                Some(*index)
            }
            _ => None,
        })
        .collect();
    assert_eq!(steps, [1, 2, 3, 4]);
    let progress: Vec<f32> = events
        .iter()
        .filter_map(|event| match event {
            CompactionEvent::Progress(value) => Some(*value),
            _ => None,
        })
        .collect();
    assert!(
        progress.windows(2).all(|pair| pair[0] <= pair[1]),
        "{progress:?}"
    );
    assert_eq!(progress.last().copied(), Some(1.0));
    let tokens = events
        .iter()
        .rev()
        .find_map(|event| match event {
            CompactionEvent::Tokens(tokens) => Some(tokens.clone()),
            _ => None,
        })
        .expect("a token breakdown");
    assert!(tokens.before_context > 0 && tokens.summarizer_input > 0);
    assert!(tokens.after_context < tokens.before_context.max(tokens.conversation_total));
    assert_eq!(tokens.window, Some(200_000));
}

#[test]
fn the_request_never_contains_the_secret_and_carries_the_system_guard() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::always(&good_reply(Technique::ClaudeCode));
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    run_ok(&request, &summarizer);
    for sent in summarizer.requests.borrow().iter() {
        assert!(!sent.user.contains(crate::common::SECRET));
        assert!(!sent.system.contains(crate::common::SECRET));
        assert!(sent.user.contains("<conversation>"));
        assert!(sent.max_output_tokens <= 500);
    }
}

#[test]
fn a_tiny_summarizer_context_forces_chunks_and_one_merge_call() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::always(&good_reply(Technique::ClaudeCode));
    let mut options = small_options(Technique::ClaudeCode);
    options.summarizer_context_tokens = 1_500;
    options.summarizer_max_output_tokens = 200;
    options.tool_result_chars = 3_000;
    options.protected_recent_tool_tokens = 10_000;
    let request = request_for(AgentKind::Claude, &path, options);
    let (outcome, _) = run_ok(&request, &summarizer);

    assert!(outcome.chunks >= 2, "chunks: {}", outcome.chunks);
    assert_eq!(
        summarizer.calls(),
        outcome.chunks + 1,
        "one call per chunk plus the merge"
    );
    let requests = summarizer.requests.borrow();
    assert!(requests.last().expect("merge").label.contains("merge"));
    assert!(requests
        .last()
        .expect("merge")
        .user
        .contains("FIRST SUMMARY MARKER"));
    // Later chunks are anchored on the running summary.
    assert!(requests[1].user.contains("<prior-summary>"));
}

#[test]
fn context_too_long_halves_the_chunk_and_retries() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::scripted(
        vec![Err(SummarizerError::ContextTooLong("too big".into()))],
        Ok(good_reply(Technique::ClaudeCode)),
    );
    let mut options = small_options(Technique::ClaudeCode);
    options.summarizer_context_tokens = 3_600;
    options.summarizer_max_output_tokens = 200;
    options.tool_result_chars = 3_000;
    options.protected_recent_tool_tokens = 10_000;
    let request = request_for(AgentKind::Claude, &path, options);
    let (outcome, events) = run_ok(&request, &summarizer);

    assert!(!outcome.used_fallback);
    assert!(
        outcome.chunks >= 2,
        "the halved budget must split the history: {}",
        outcome.chunks
    );
    assert!(
        events.iter().any(|event| matches!(event, CompactionEvent::Log(line) if line.contains("retrying with chunks of about"))),
        "the rejection must be logged"
    );
}

#[test]
fn a_malformed_reply_is_retried_once_with_the_reason() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::scripted(
        vec![Ok("Sure, here is nothing useful.".into())],
        Ok(good_reply(Technique::ClaudeCode)),
    );
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    let (outcome, _) = run_ok(&request, &summarizer);

    assert!(!outcome.used_fallback);
    assert_eq!(summarizer.calls(), 2);
    let requests = summarizer.requests.borrow();
    assert_ne!(
        requests[0].user, requests[1].user,
        "the retry carries a note"
    );
}

#[test]
fn a_summarizer_that_never_works_yields_a_fallback_summary_in_the_file() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Claude);
    let summarizer = Fake::failing();
    let request = request_for(
        AgentKind::Claude,
        &path,
        small_options(Technique::ClaudeCode),
    );
    let (outcome, events) = run_ok(&request, &summarizer);

    assert!(outcome.used_fallback);
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("isCompactSummary"));
    assert!(
        text.contains("add a verbose flag") || text.contains("verbose"),
        "last user requests are in the fallback"
    );
    assert!(events.iter().any(|event| matches!(event, CompactionEvent::Log(line) if line.to_lowercase().contains("fallback"))));
}

#[test]
fn cancellation_during_a_call_leaves_the_original_untouched() {
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        let dir = tempfile::tempdir().expect("dir");
        let path = install_fixture(dir.path(), agent);
        let original = std::fs::read(&path).expect("read");
        let cancel = AtomicBool::new(false);
        let summarizer = Fake::always(&good_reply(Technique::default_for(agent)))
            .on_call(|| cancel.store(true, Ordering::SeqCst));
        let request = request_for(agent, &path, small_options(Technique::default_for(agent)));
        let finished = run(&request, &summarizer, &cancel);

        assert!(
            matches!(finished.result, Err(CompactionError::Cancelled)),
            "{agent:?}"
        );
        assert_eq!(std::fs::read(&path).expect("read"), original);
        assert_eq!(
            dir_listing(dir.path()).len(),
            1,
            "no backup or temp file may remain"
        );
    }
}

#[test]
fn cancellation_before_start_makes_no_call() {
    let dir = tempfile::tempdir().expect("dir");
    let path = install_fixture(dir.path(), AgentKind::Codex);
    let summarizer = Fake::always("unused");
    let request = request_for(AgentKind::Codex, &path, small_options(Technique::Codex));
    let finished = run(&request, &summarizer, &AtomicBool::new(true));
    assert!(matches!(finished.result, Err(CompactionError::Cancelled)));
    assert_eq!(summarizer.calls(), 0);
}

#[test]
fn every_technique_compacts_both_agents() {
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        for technique in Technique::ALL {
            let dir = tempfile::tempdir().expect("dir");
            let path = install_fixture(dir.path(), agent);
            let summarizer = Fake::always(&good_reply(technique));
            let mut options = small_options(technique);
            if technique == Technique::Custom {
                options.custom_prompt = Some("Summarize in two lines.\n\n{{conversation}}".into());
            }
            let request = request_for(agent, &path, options);
            let (outcome, _) = run_ok(&request, &summarizer);
            assert!(!outcome.used_fallback, "{agent:?} {technique:?}");
        }
    }
}

#[test]
fn pause_point_and_context_usage_read_the_fixtures() {
    let dir = tempfile::tempdir().expect("dir");
    let claude = install_fixture(dir.path(), AgentKind::Claude);
    let codex = install_fixture(dir.path(), AgentKind::Codex);
    // Both fixtures end in an unanswered tool call, so neither is at a pause.
    assert!(!transcript_is_at_pause_point(AgentKind::Claude, &claude));
    assert!(!transcript_is_at_pause_point(AgentKind::Codex, &codex));
    assert!(!transcript_is_at_pause_point(
        AgentKind::Codex,
        &dir.path().join("missing.jsonl")
    ));

    assert_eq!(
        latest_context_usage(AgentKind::Codex, &codex),
        Some((93_100, Some(258_400)))
    );
    let (claude_tokens, _) = latest_context_usage(AgentKind::Claude, &claude).expect("usage");
    assert!(claude_tokens > 0);
}
