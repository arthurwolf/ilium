use super::fixtures::*;
use super::listing::list_transcripts;
use super::stream::{parse_file, read_bounded_line, FileOutcome, LineRead};
use super::*;
use crate::compaction_report::ReportStatus;
use std::cell::Cell;
use std::io::Cursor;
use std::time::SystemTime;

fn never_stop() -> bool {
    false
}

fn scan(
    agent: AgentKind,
    home: &Path,
    cache: Option<&Path>,
) -> (ScanOutput, Arc<ScanProgressCounters>) {
    let counters = Arc::new(ScanProgressCounters::default());
    let output =
        scan_corpus(agent, home, cache, usize::MAX, &counters, &never_stop).expect("scan finishes");
    (output, counters)
}

fn set_mtime(path: &Path, time: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime")
        .set_modified(time)
        .expect("set mtime");
}

// ------------------------------------------------------------------ listing

#[test]
fn claude_listing_finds_main_subagent_and_workflow_transcripts() {
    let home = tempfile::tempdir().unwrap();
    let process = Process::claude();
    let text = claude_session(&process, 0, "a");
    write(&claude_main_path(home.path(), "-proj-a", "main-1"), &text);
    write(&claude_main_path(home.path(), "-proj-b", "main-2"), &text);
    write(
        &claude_subagent_path(home.path(), "-proj-a", "main-1", "agent-x"),
        &text,
    );
    write(
        &claude_workflow_path(home.path(), "-proj-a", "main-1", "flow-y"),
        &text,
    );
    // Not transcripts: wrong extension, empty file.
    write(&home.path().join(".claude/projects/-proj-a/notes.txt"), "x");
    write(&claude_main_path(home.path(), "-proj-a", "empty"), "");

    let counters = ScanProgressCounters::default();
    let listing = list_transcripts(AgentKind::ClaudeCode, home.path(), &counters, &never_stop);
    assert_eq!(listing.files.len(), 4);
    let subagents = listing.files.iter().filter(|file| file.is_subagent).count();
    assert_eq!(subagents, 2, "subagents/ and workflows/ are flagged");
    assert_eq!(counters.files_found(), 4);
    assert_eq!(counters.bytes_total(), listing.bytes_total);
    assert_eq!(listing.bytes_total, 4 * text.len() as u64);
}

#[test]
fn codex_listing_finds_rollouts_and_archived_sessions_only() {
    let home = tempfile::tempdir().unwrap();
    let text = codex_session(&Process::codex(), 0, "a");
    write(&codex_path(home.path(), "one"), &text);
    write(&codex_path(home.path(), "two"), &text);
    write(&codex_archived_path(home.path(), "old"), &text);
    write(
        &home.path().join(".codex/sessions/2026/09/30/other.jsonl"),
        &text,
    );
    // A Claude tree is not Codex's business.
    write(&claude_main_path(home.path(), "-proj", "main"), &text);

    let counters = ScanProgressCounters::default();
    let listing = list_transcripts(AgentKind::Codex, home.path(), &counters, &never_stop);
    assert_eq!(listing.files.len(), 3);
    assert!(listing.files.iter().all(|file| !file.is_subagent));
}

#[test]
fn listing_a_missing_home_is_empty_not_an_error() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("does-not-exist");
    for agent in AgentKind::ALL {
        let counters = ScanProgressCounters::default();
        let listing = list_transcripts(agent, &missing, &counters, &never_stop);
        assert!(listing.files.is_empty());
        assert!(listing.warnings.is_empty());
        assert!(!listing.was_stopped);
    }
}

#[test]
fn listing_observes_a_stop_request() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 2);
    let counters = ScanProgressCounters::default();
    let listing = list_transcripts(AgentKind::ClaudeCode, home.path(), &counters, &|| true);
    assert!(listing.was_stopped);
}

// ------------------------------------------------------------------ reading

#[test]
fn bounded_line_reader_cuts_oversize_lines_and_keeps_the_next_one() {
    let input = b"short\nthis line is far too long\nnext\nlast without newline";
    let mut reader = Cursor::new(&input[..]);
    let mut buffer = Vec::new();
    let mut consumed = 0_usize;
    let mut on_chunk = |bytes: usize| {
        consumed += bytes;
        true
    };
    let mut read = |buffer: &mut Vec<u8>, on_chunk: &mut dyn FnMut(usize) -> bool| {
        read_bounded_line(&mut reader, buffer, 8, on_chunk).unwrap()
    };
    assert_eq!(
        read(&mut buffer, &mut on_chunk),
        LineRead::Line { oversize: false }
    );
    assert_eq!(buffer, b"short\n");
    assert_eq!(
        read(&mut buffer, &mut on_chunk),
        LineRead::Line { oversize: true }
    );
    assert_eq!(
        buffer.len(),
        9,
        "limit plus one byte, so the parser counts it"
    );
    assert_eq!(
        read(&mut buffer, &mut on_chunk),
        LineRead::Line { oversize: false }
    );
    assert_eq!(buffer, b"next\n");
    assert_eq!(
        read(&mut buffer, &mut on_chunk),
        LineRead::Line { oversize: true }
    );
    assert_eq!(
        buffer, b"last with",
        "an unterminated last line is still returned"
    );
    assert_eq!(read(&mut buffer, &mut on_chunk), LineRead::Eof);
    assert_eq!(consumed, input.len(), "every byte is reported as progress");
}

#[test]
fn bounded_line_reader_stops_when_the_callback_says_so() {
    let mut reader = Cursor::new(&b"line one\nline two\n"[..]);
    let mut buffer = Vec::new();
    let outcome = read_bounded_line(&mut reader, &mut buffer, 1024, &mut |_| false).unwrap();
    assert_eq!(outcome, LineRead::Stopped);
}

#[test]
fn a_truncated_trailing_line_is_tolerated_and_counted() {
    let home = tempfile::tempdir().unwrap();
    let mut text = claude_session(&Process::claude(), 0, "t");
    // A live session cut mid-write: the last request loses its end.
    text.push_str(r#"{"type":"assistant","message":{"id":"msg_cut","usage":{"input_tok"#);
    let path = write(&claude_main_path(home.path(), "-proj", "live"), &text);
    let size = std::fs::metadata(&path).unwrap().len();

    let mut bytes_seen = 0_u64;
    let outcome = parse_file(AgentKind::ClaudeCode, &path, size, false, &mut |bytes| {
        bytes_seen += bytes as u64;
        true
    });
    let FileOutcome::Parsed { trace } = outcome else {
        panic!("the file parses");
    };
    assert_eq!(bytes_seen, size, "every byte is reported as progress");
    assert!(trace.counters.lines_unparseable >= 1);
    assert!(trace.turns.len() >= 100, "the complete lines still count");
}

#[test]
fn only_the_listed_prefix_of_a_growing_file_is_read() {
    let home = tempfile::tempdir().unwrap();
    let text = claude_session(&Process::claude(), 0, "g");
    let path = write(&claude_main_path(home.path(), "-proj", "grow"), &text);
    let listed_size = std::fs::metadata(&path).unwrap().len();
    // The session appends after the listing.
    let mut grown = text.clone();
    grown.push_str(&claude_session(&Process::claude(), 1, "g2"));
    std::fs::write(&path, grown).unwrap();
    let mut bytes_seen = 0_u64;
    let outcome = parse_file(
        AgentKind::ClaudeCode,
        &path,
        listed_size,
        false,
        &mut |bytes| {
            bytes_seen += bytes as u64;
            true
        },
    );
    assert!(matches!(outcome, FileOutcome::Parsed { .. }));
    assert_eq!(bytes_seen, listed_size);
}

#[test]
fn a_vanished_file_is_unreadable_and_progress_still_reaches_its_size() {
    let home = tempfile::tempdir().unwrap();
    let listed = ListedFile {
        path: home.path().join("gone.jsonl"),
        size: 4096,
        mtime_ms: 0,
        is_subagent: false,
    };
    let counters = ScanProgressCounters::default();
    let mut since_pause = 0;
    let outcome = read_file(
        AgentKind::ClaudeCode,
        &listed,
        &counters,
        &never_stop,
        &mut since_pause,
    );
    assert!(matches!(outcome, FileOutcome::Unreadable(_)));
    assert_eq!(counters.bytes_done(), 4096);
}

#[test]
fn a_shrunk_file_still_lets_progress_reach_the_listed_size() {
    let home = tempfile::tempdir().unwrap();
    let text = claude_session(&Process::claude(), 0, "s");
    let path = write(&claude_main_path(home.path(), "-proj", "shrunk"), &text);
    let listed = ListedFile {
        size: text.len() as u64 + 10_000,
        path,
        mtime_ms: 0,
        is_subagent: false,
    };
    let counters = ScanProgressCounters::default();
    let mut since_pause = 0;
    let outcome = read_file(
        AgentKind::ClaudeCode,
        &listed,
        &counters,
        &never_stop,
        &mut since_pause,
    );
    assert!(matches!(outcome, FileOutcome::Parsed { .. }));
    assert_eq!(counters.bytes_done(), listed.size);
    assert!(file_changed_since_listing(&listed));
}

// -------------------------------------------------------------- scan_corpus

#[test]
fn byte_progress_reaches_the_total_and_file_counts_match() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 4);
    write(
        &claude_subagent_path(home.path(), "-synthetic-project", "session-00", "agent-a"),
        &claude_session(&Process::claude(), 9, "sub"),
    );
    let (output, counters) = scan(AgentKind::ClaudeCode, home.path(), None);
    assert_eq!(counters.files_total(), 5);
    assert_eq!(counters.files_done(), 5);
    assert_eq!(counters.bytes_done(), counters.bytes_total());
    assert!(counters.bytes_total() > 0);
    assert_eq!(counters.files_parsed(), 5);
    assert_eq!(counters.phase(), ScanPhase::Analyzing);
    assert_eq!(output.summary.files_listed, 5);
    assert_eq!(output.traces.len(), 5);
    // Main sessions come first, the subagent last.
    assert!(output.traces[..4].iter().all(|trace| !trace.is_subagent));
    assert!(output.traces[4].is_subagent);
}

#[test]
fn codex_scan_parses_rollouts_and_archived_sessions() {
    let home = tempfile::tempdir().unwrap();
    write_codex_corpus(home.path(), 3);
    write(
        &codex_archived_path(home.path(), "archived"),
        &codex_session(&Process::codex(), 7, "arch"),
    );
    let (output, counters) = scan(AgentKind::Codex, home.path(), None);
    assert_eq!(output.traces.len(), 4);
    assert_eq!(counters.bytes_done(), counters.bytes_total());
    assert!(output
        .traces
        .iter()
        .all(|trace| trace.agent == AgentKind::Codex && !trace.compactions.is_empty()));
}

#[test]
fn missing_home_and_empty_corpus_scan_to_empty_traces() {
    let home = tempfile::tempdir().unwrap();
    for agent in AgentKind::ALL {
        let (output, counters) = scan(agent, &home.path().join("nowhere"), None);
        assert!(output.traces.is_empty());
        assert_eq!(output.summary.files_listed, 0);
        assert_eq!(counters.files_total(), 0);
    }
}

#[test]
fn cross_file_duplicates_are_removed_once() {
    let home = tempfile::tempdir().unwrap();
    // A resumed session copies its history into a second file.
    let text = claude_session(&Process::claude(), 0, "same");
    write(&claude_main_path(home.path(), "-proj", "a-original"), &text);
    write(&claude_main_path(home.path(), "-proj", "b-resumed"), &text);
    let (output, _) = scan(AgentKind::ClaudeCode, home.path(), None);
    assert!(output.summary.duplicate_turns_removed > 100);
    assert!(output.traces[1].turns.is_empty());
}

#[test]
fn a_stop_during_the_read_cancels_and_leaves_the_cache_untouched() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 3);
    let cache = home.path().join("cache").join("traces.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, b"previous-cache-bytes").unwrap();

    let polls = Cell::new(0_u32);
    let counters = ScanProgressCounters::default();
    let result = scan_corpus(
        AgentKind::ClaudeCode,
        home.path(),
        Some(&cache),
        usize::MAX,
        &counters,
        &|| {
            polls.set(polls.get() + 1);
            polls.get() > 6
        },
    );
    assert_eq!(result.unwrap_err(), ScanCancelled);
    assert!(counters.bytes_done() < counters.bytes_total());
    assert_eq!(std::fs::read(&cache).unwrap(), b"previous-cache-bytes");
}

#[test]
fn a_stop_before_anything_ends_the_scan_without_panicking() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 1);
    let counters = ScanProgressCounters::default();
    let result = scan_corpus(
        AgentKind::ClaudeCode,
        home.path(),
        None,
        usize::MAX,
        &counters,
        &|| true,
    );
    assert_eq!(result.unwrap_err(), ScanCancelled);
}

// -------------------------------------------------------------------- cache

#[test]
fn an_unchanged_file_is_served_from_the_cache_without_parsing() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 2);
    let cache = home.path().join("cache").join("traces.json");
    let (first, first_counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(first_counters.files_parsed(), 2);
    assert_eq!(first_counters.files_from_cache(), 0);
    assert!(cache.is_file());

    // Replace the content with different text of the same length and restore
    // the modification time: only a cache hit can return the old trace.
    let path = claude_main_path(home.path(), "-synthetic-project", "session-00");
    let original = std::fs::read(&path).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, vec![b'x'; original.len()]).unwrap();
    set_mtime(&path, mtime);

    let (second, second_counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(second_counters.files_parsed(), 0, "nothing was re-parsed");
    assert_eq!(second_counters.files_from_cache(), 2);
    assert_eq!(second.summary.files_from_cache, 2);
    assert_eq!(
        second_counters.bytes_done(),
        second_counters.bytes_total(),
        "cached bytes count as progress"
    );
    assert_eq!(first.traces, second.traces);
}

#[test]
fn a_changed_size_or_mtime_invalidates_only_that_file() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 3);
    let cache = home.path().join("cache").join("traces.json");
    scan(AgentKind::ClaudeCode, home.path(), Some(&cache));

    // Size change: the session appended a request.
    let grown = claude_main_path(home.path(), "-synthetic-project", "session-00");
    let mut text = std::fs::read_to_string(&grown).unwrap();
    text.push_str(&claude_session(&Process::claude(), 20, "extra"));
    std::fs::write(&grown, text).unwrap();
    // mtime change only: same bytes, later modification time.
    let touched = claude_main_path(home.path(), "-synthetic-project", "session-01");
    set_mtime(&touched, SystemTime::now() + Duration::from_secs(3600));

    let (_, counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(counters.files_parsed(), 2);
    assert_eq!(counters.files_from_cache(), 1);

    // The refreshed cache serves all three on the next scan.
    let (_, third) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(third.files_parsed(), 0);
    assert_eq!(third.files_from_cache(), 3);
}

#[test]
fn a_cache_of_another_layout_or_agent_is_ignored() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 2);
    let cache = home.path().join("cache").join("traces.json");
    scan(AgentKind::ClaudeCode, home.path(), Some(&cache));

    // Another cache layout version.
    let text = std::fs::read_to_string(&cache).unwrap();
    let other_version = text.replacen("\"cache_version\":1", "\"cache_version\":999", 1);
    assert_ne!(text, other_version);
    std::fs::write(&cache, other_version).unwrap();
    let (_, counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(counters.files_parsed(), 2);

    // Another trace format version.
    let text = std::fs::read_to_string(&cache).unwrap();
    let other_format = text.replacen("\"trace_format_version\":", "\"trace_format_version\":9", 1);
    std::fs::write(&cache, other_format).unwrap();
    let (_, counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(counters.files_parsed(), 2);

    // The previous trace format (1) predates the current one (2): rescan.
    assert_eq!(ilium_compaction_analysis::TRACE_FORMAT_VERSION, 2);
    let text = std::fs::read_to_string(&cache).unwrap();
    let previous_format = text.replacen(
        "\"trace_format_version\":2",
        "\"trace_format_version\":1",
        1,
    );
    assert_ne!(text, previous_format);
    std::fs::write(&cache, previous_format).unwrap();
    let (_, counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(counters.files_parsed(), 2);
    assert_eq!(counters.files_from_cache(), 0);

    // Garbage.
    std::fs::write(&cache, b"not json at all").unwrap();
    let (_, counters) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(counters.files_parsed(), 2);
}

#[test]
fn a_cache_that_cannot_be_written_is_a_warning_not_a_failure() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 1);
    // The cache path's parent is a regular file.
    let blocker = home.path().join("blocker");
    std::fs::write(&blocker, b"file").unwrap();
    let cache = blocker.join("traces.json");
    let (output, _) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(output.traces.len(), 1);
    assert!(output
        .summary
        .warnings
        .iter()
        .any(|warning| warning.contains("trace cache was not updated")));
}

#[test]
fn default_cache_paths_are_separate_per_agent_and_versioned() {
    let claude = cache_file_name(AgentKind::ClaudeCode);
    let codex = cache_file_name(AgentKind::Codex);
    assert_ne!(claude, codex);
    assert!(claude.contains("claude_code") && claude.ends_with("-v1.json"));
}

#[test]
fn the_retention_cap_keeps_the_most_recent_sessions() {
    let process = Process::claude();
    let mut scanned = Vec::new();
    for index in 0..4_i64 {
        let home = tempfile::tempdir().unwrap();
        write(
            &claude_main_path(home.path(), "-p", "s"),
            &claude_session(&process, index as usize, "cap"),
        );
        let (output, _) = scan(AgentKind::ClaudeCode, home.path(), None);
        scanned.push(ScannedFile {
            listed: ListedFile {
                path: PathBuf::from(format!("/synthetic/{index}.jsonl")),
                size: 1,
                mtime_ms: index * 1000,
                is_subagent: false,
            },
            trace: output.traces.into_iter().next().unwrap(),
            is_cacheable: true,
        });
    }
    let one = cache::retained_bytes(&scanned[0].trace);
    let dropped = apply_retention_cap(&mut scanned, one * 2 + one / 2);
    assert_eq!(dropped, 2);
    let mut kept: Vec<i64> = scanned.iter().map(|file| file.listed.mtime_ms).collect();
    kept.sort_unstable();
    assert_eq!(kept, vec![2000, 3000], "the newest sessions survive");
    assert_eq!(apply_retention_cap(&mut scanned, usize::MAX), 0);
}

// ----------------------------------------------------- optimizer (real job)

fn start(
    optimizer: &mut CompactionOptimizer,
    agent: AgentKind,
    home: &Path,
    cache_dir: Option<PathBuf>,
    settings: ScanSettingsInput,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    // The shared test bank can be briefly busy with other fixtures' jobs.
    while !optimizer.start_scan(
        agent,
        home.to_path_buf(),
        cache_dir.clone(),
        settings.clone(),
    ) {
        assert!(Instant::now() < deadline, "scan admission never succeeded");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Drains until the scan finishes; returns whether any drain reported a change.
fn finish(optimizer: &mut CompactionOptimizer, agent: AgentKind) -> bool {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut changed = false;
    while optimizer.is_scanning(agent) {
        assert!(Instant::now() < deadline, "scan did not finish");
        changed |= optimizer.drain_events(Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
    changed
}

#[test]
fn drain_events_delivers_a_claude_report_through_the_execution_client() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 6);
    let mut optimizer = CompactionOptimizer::new();
    assert!(matches!(
        optimizer.view(AgentKind::ClaudeCode),
        ScanView::Idle
    ));
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    assert!(optimizer.is_scanning(AgentKind::ClaudeCode));
    assert!(
        !optimizer.start_scan(
            AgentKind::ClaudeCode,
            home.path().to_path_buf(),
            None,
            ScanSettingsInput::default()
        ),
        "a second scan of the same agent is refused while one runs"
    );
    assert!(!optimizer.is_scanning(AgentKind::Codex));
    assert!(
        finish(&mut optimizer, AgentKind::ClaudeCode),
        "finishing is a change"
    );
    let ScanView::Ready(report) = optimizer.view(AgentKind::ClaudeCode) else {
        panic!("a finished scan is ready");
    };
    assert_eq!(report.status, ReportStatus::Complete);
    assert_eq!(report.corpus.files_listed, 6);
    assert_eq!(report.corpus.main_sessions, 6);
    let recommendation = report.recommendation.as_ref().expect("a recommendation");
    assert_eq!(recommendation.setting_key, "autoCompactWindow");
    assert!(report
        .to_plain_text()
        .contains("Optimal: autoCompactWindow"));
    assert!(optimizer.report(AgentKind::ClaudeCode).is_some());
    assert!(matches!(optimizer.view(AgentKind::Codex), ScanView::Idle));
    assert!(
        !optimizer.drain_events(Instant::now()),
        "nothing is left to report"
    );
}

#[test]
fn a_codex_scan_runs_independently_and_reports_its_own_setting() {
    let home = tempfile::tempdir().unwrap();
    write_codex_corpus(home.path(), 6);
    let mut optimizer = CompactionOptimizer::new();
    start(
        &mut optimizer,
        AgentKind::Codex,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    finish(&mut optimizer, AgentKind::Codex);
    let report = optimizer.report(AgentKind::Codex).expect("a codex report");
    assert_eq!(report.status, ReportStatus::Complete);
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.setting_key, "model_auto_compact_token_limit");
    assert!(card.setting_value <= 232_560);
    assert!(optimizer.report(AgentKind::ClaudeCode).is_none());
}

#[test]
fn a_missing_home_gives_a_clear_no_sessions_report() {
    let home = tempfile::tempdir().unwrap();
    let mut optimizer = CompactionOptimizer::new();
    for agent in AgentKind::ALL {
        start(
            &mut optimizer,
            agent,
            &home.path().join("does-not-exist"),
            None,
            ScanSettingsInput::default(),
        );
        finish(&mut optimizer, agent);
        let ScanView::Ready(report) = optimizer.view(agent) else {
            panic!("an empty corpus is a result, not a failure");
        };
        assert_eq!(report.status, ReportStatus::NoSessionsFound);
        assert!(report.recommendation.is_none());
        assert!(report
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("No sessions found")));
    }
}

#[test]
fn the_trace_cache_makes_the_second_job_scan_parse_nothing() {
    let home = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 6);
    let mut optimizer = CompactionOptimizer::new();
    for expected_cached in [0, 6] {
        start(
            &mut optimizer,
            AgentKind::ClaudeCode,
            home.path(),
            Some(cache_dir.path().to_path_buf()),
            ScanSettingsInput::default(),
        );
        finish(&mut optimizer, AgentKind::ClaudeCode);
        let report = optimizer.report(AgentKind::ClaudeCode).unwrap();
        assert_eq!(report.corpus.files_from_cache, expected_cached);
        assert_eq!(report.corpus.files_parsed, 6 - expected_cached);
    }
    assert!(cache_dir
        .path()
        .join(cache_file_name(AgentKind::ClaudeCode))
        .is_file());
}

#[test]
fn cancelling_keeps_the_previous_report() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 6);
    let mut optimizer = CompactionOptimizer::new();
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    finish(&mut optimizer, AgentKind::ClaudeCode);
    let previous = optimizer
        .report(AgentKind::ClaudeCode)
        .unwrap()
        .generated_at_unix_ms;

    // A much larger corpus keeps the second scan busy long enough that the
    // cancel request, sent right after the submit, always arrives first.
    for index in 6..300 {
        write(
            &claude_main_path(
                home.path(),
                "-synthetic-project",
                &format!("session-{index:03}"),
            ),
            &claude_session(&Process::claude(), index, "more"),
        );
    }
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    optimizer.cancel_scan(AgentKind::ClaudeCode);
    assert!(finish(&mut optimizer, AgentKind::ClaudeCode));
    assert!(matches!(
        optimizer.view(AgentKind::ClaudeCode),
        ScanView::Cancelled
    ));
    let report = optimizer
        .report(AgentKind::ClaudeCode)
        .expect("the previous report survives a cancelled scan");
    assert_eq!(report.generated_at_unix_ms, previous);
    assert_eq!(report.corpus.files_listed, 6, "it is the old report");
    assert!(!optimizer.is_scanning(AgentKind::ClaudeCode));
    // A new scan can start again afterwards and replaces the report.
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    finish(&mut optimizer, AgentKind::ClaudeCode);
    assert_eq!(
        optimizer
            .report(AgentKind::ClaudeCode)
            .unwrap()
            .corpus
            .files_listed,
        300
    );
}

#[test]
fn progress_is_reported_while_a_scan_runs() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 40);
    let mut optimizer = CompactionOptimizer::new();
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput::default(),
    );
    let mut saw_running_view = false;
    let mut ticks = 0;
    let deadline = Instant::now() + Duration::from_secs(120);
    while optimizer.is_scanning(AgentKind::ClaudeCode) {
        assert!(Instant::now() < deadline);
        if optimizer.drain_events(Instant::now()) {
            ticks += 1;
        }
        match optimizer.view(AgentKind::ClaudeCode) {
            ScanView::Listing { .. } => saw_running_view = true,
            ScanView::Scanning(progress) => {
                saw_running_view = true;
                assert!(progress.bytes_done <= progress.bytes_total);
                assert!(progress.fraction() <= 1.0);
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(ticks >= 1, "at least the finish is reported");
    assert!(saw_running_view || optimizer.report(AgentKind::ClaudeCode).is_some());
}

#[test]
fn a_current_setting_adds_the_three_way_comparison() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 6);
    let mut optimizer = CompactionOptimizer::new();
    start(
        &mut optimizer,
        AgentKind::ClaudeCode,
        home.path(),
        None,
        ScanSettingsInput {
            current_setting_value: Some(250_000),
            ..ScanSettingsInput::default()
        },
    );
    finish(&mut optimizer, AgentKind::ClaudeCode);
    let report = optimizer.report(AgentKind::ClaudeCode).unwrap();
    assert_eq!(report.comparison.len(), 3);
    let current = report
        .comparison
        .iter()
        .find(|row| row.kind == crate::compaction_report::ComparisonKind::CurrentSetting)
        .unwrap();
    assert_eq!(current.setting_value, Some(250_000));
    assert_eq!(current.trigger_tokens, 217_000);
}

#[test]
fn without_a_client_the_scan_reports_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let mut optimizer = CompactionOptimizer::new();
    optimizer.client = None;
    assert!(!optimizer.start_scan(
        AgentKind::Codex,
        home.path().to_path_buf(),
        None,
        ScanSettingsInput::default()
    ));
    assert!(matches!(
        optimizer.view(AgentKind::Codex),
        ScanView::Failed(message) if message.contains("unavailable")
    ));
}

#[test]
fn sessions_dropped_for_the_cap_stay_in_the_cache() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 3);
    let cache = home.path().join("cache").join("traces.json");
    let counters = ScanProgressCounters::default();
    // A cap that holds a single session: two are dropped from the result...
    let (probe, _) = scan(AgentKind::ClaudeCode, home.path(), None);
    let one = cache::retained_bytes(&probe.traces[0]);
    let one_session = one + one / 2;
    let output = scan_corpus(
        AgentKind::ClaudeCode,
        home.path(),
        Some(&cache),
        one_session,
        &counters,
        &never_stop,
    )
    .expect("scan finishes");
    assert_eq!(output.summary.sessions_dropped_for_cap, 2);
    assert_eq!(output.traces.len(), 1);
    // ...but the cache keeps all three, so the next scan parses nothing.
    assert_eq!(cache::load(&cache, AgentKind::ClaudeCode).len(), 3);
    let (_, second) = scan(AgentKind::ClaudeCode, home.path(), Some(&cache));
    assert_eq!(second.files_parsed(), 0);
    assert_eq!(second.files_from_cache(), 3);
}
