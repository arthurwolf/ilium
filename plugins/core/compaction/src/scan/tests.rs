use super::fixtures::*;
use super::listing::list_transcripts;
use super::source::ScanPath;
use super::stream::{parse_file, read_bounded_line, FileOutcome, LineRead};
use super::test_fs::StdFs;
use super::*;
use std::cell::Cell;
use std::io::Cursor;
use std::path::Path;

fn never_stop() -> bool {
    false
}

fn scan(agent: AgentKind, home: &Path) -> (ScanOutput, ScanProgressCounters) {
    let counters = ScanProgressCounters::default();
    let output = scan_corpus(agent, &StdFs::new(home), usize::MAX, &counters, &never_stop)
        .expect("scan finishes");
    (output, counters)
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
    let listing = list_transcripts(
        AgentKind::ClaudeCode,
        &StdFs::new(home.path()),
        &counters,
        &never_stop,
    );
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
    let listing = list_transcripts(
        AgentKind::Codex,
        &StdFs::new(home.path()),
        &counters,
        &never_stop,
    );
    assert_eq!(listing.files.len(), 3);
    assert!(listing.files.iter().all(|file| !file.is_subagent));
}

#[test]
fn listing_a_missing_home_is_empty_not_an_error() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("does-not-exist");
    for agent in AgentKind::ALL {
        let counters = ScanProgressCounters::default();
        let listing = list_transcripts(agent, &StdFs::new(&missing), &counters, &never_stop);
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
    let listing = list_transcripts(
        AgentKind::ClaudeCode,
        &StdFs::new(home.path()),
        &counters,
        &|| true,
    );
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
    let fs = StdFs::new(home.path());
    let outcome = parse_file(
        AgentKind::ClaudeCode,
        &fs,
        &fs.scan_path(&path),
        size,
        false,
        &mut |bytes| {
            bytes_seen += bytes as u64;
            true
        },
    );
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
    let fs = StdFs::new(home.path());
    let outcome = parse_file(
        AgentKind::ClaudeCode,
        &fs,
        &fs.scan_path(&path),
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
    let fs = StdFs::new(home.path());
    let listed = ListedFile {
        path: ScanPath::new("agent-sessions:claude", "gone.jsonl"),
        size: 4096,
        mtime_ms: 0,
        is_subagent: false,
    };
    let counters = ScanProgressCounters::default();
    let outcome = read_file(AgentKind::ClaudeCode, &fs, &listed, &counters, &never_stop);
    assert!(matches!(outcome, FileOutcome::Unreadable(_)));
    assert_eq!(counters.bytes_done(), 4096);
}

#[test]
fn a_shrunk_file_still_lets_progress_reach_the_listed_size() {
    let home = tempfile::tempdir().unwrap();
    let text = claude_session(&Process::claude(), 0, "s");
    let path = write(&claude_main_path(home.path(), "-proj", "shrunk"), &text);
    let fs = StdFs::new(home.path());
    let listed = ListedFile {
        size: text.len() as u64 + 10_000,
        path: fs.scan_path(&path),
        mtime_ms: 0,
        is_subagent: false,
    };
    let counters = ScanProgressCounters::default();
    let outcome = read_file(AgentKind::ClaudeCode, &fs, &listed, &counters, &never_stop);
    assert!(matches!(outcome, FileOutcome::Parsed { .. }));
    assert_eq!(counters.bytes_done(), listed.size);
    assert!(file_changed_since_listing(&fs, &listed));
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
    let (output, counters) = scan(AgentKind::ClaudeCode, home.path());
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
    let (output, counters) = scan(AgentKind::Codex, home.path());
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
        let (output, counters) = scan(agent, &home.path().join("nowhere"));
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
    let (output, _) = scan(AgentKind::ClaudeCode, home.path());
    assert!(output.summary.duplicate_turns_removed > 100);
    assert!(output.traces[1].turns.is_empty());
}

#[test]
fn a_stop_during_the_read_cancels_the_scan() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 3);
    let polls = Cell::new(0_u32);
    let counters = ScanProgressCounters::default();
    let result = scan_corpus(
        AgentKind::ClaudeCode,
        &StdFs::new(home.path()),
        usize::MAX,
        &counters,
        &|| {
            polls.set(polls.get() + 1);
            polls.get() > 6
        },
    );
    assert_eq!(result.unwrap_err(), ScanCancelled);
    assert!(counters.bytes_done() < counters.bytes_total());
}

#[test]
fn a_stop_before_anything_ends_the_scan_without_panicking() {
    let home = tempfile::tempdir().unwrap();
    write_claude_corpus(home.path(), 1);
    let counters = ScanProgressCounters::default();
    let result = scan_corpus(
        AgentKind::ClaudeCode,
        &StdFs::new(home.path()),
        usize::MAX,
        &counters,
        &|| true,
    );
    assert_eq!(result.unwrap_err(), ScanCancelled);
}

// ---------------------------------------------------------- retention cap

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
        let (output, _) = scan(AgentKind::ClaudeCode, home.path());
        scanned.push(ScannedFile {
            listed: ListedFile {
                path: ScanPath::new("agent-sessions:claude", &format!("synthetic/{index}.jsonl")),
                size: 1,
                mtime_ms: index * 1000,
                is_subagent: false,
            },
            trace: output.traces.into_iter().next().unwrap(),
        });
    }
    let one = retained_bytes(&scanned[0].trace);
    let dropped = apply_retention_cap(&mut scanned, one * 2 + one / 2);
    assert_eq!(dropped, 2);
    let mut kept: Vec<i64> = scanned.iter().map(|file| file.listed.mtime_ms).collect();
    kept.sort_unstable();
    assert_eq!(kept, vec![2000, 3000], "the newest sessions survive");
    assert_eq!(apply_retention_cap(&mut scanned, usize::MAX), 0);
}
