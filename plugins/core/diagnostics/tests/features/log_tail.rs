//! Tail the log file (page sample "Tail the log file"): a cursor, chunks of 64 KiB, only WARN and ERROR
//! lines, a list of at most 200 lines and the marker for skipped lines.

use crate::common::{append_log, log_line, session, warnings_panel, write_log};

const LOG: &str = "log-2026-10-12 09-14-03.txt";

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

/// REQ: COR-050
#[test]
fn cor_050_a_warn_line_of_the_log_reaches_the_warnings_panel() {
    let mut session = session();
    write_log(&mut session, LOG, log_line("WARN", "disk almost full").as_bytes());
    session.pump().expect("session settles");
    assert!(warnings_panel(&mut session, 100, 20).contains("disk almost full"));
}

/// REQ: COR-050
#[test]
fn cor_050_an_error_line_of_the_log_reaches_the_warnings_panel() {
    let mut session = session();
    write_log(&mut session, LOG, log_line("ERROR", "model call failed").as_bytes());
    session.pump().expect("session settles");
    assert!(warnings_panel(&mut session, 100, 20).contains("model call failed"));
}

/// REQ: COR-050
#[test]
fn cor_050_info_debug_and_trace_lines_do_not_reach_the_warnings_panel() {
    let mut session = session();
    let text = [log_line("INFO", "started"), log_line("DEBUG", "tick"), log_line("TRACE", "poll")].concat();
    write_log(&mut session, LOG, text.as_bytes());
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    for word in ["started", "tick", "poll"] {
        assert!(!panel.contains(word), "{panel}");
    }
}

/// REQ: COR-050
#[test]
fn cor_050_a_line_needs_the_level_word_between_spaces() {
    let mut session = session();
    write_log(&mut session, LOG, b"2026-10-12 09:14:03 WARNING not a level word\n2026-10-12 09:14:03 warn lower case\n");
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert!(!panel.contains("not a level word"), "{panel}");
    assert!(!panel.contains("lower case"), "{panel}");
}

/// REQ: COR-050
#[test]
fn cor_050_the_second_poll_reads_only_the_bytes_after_the_cursor() {
    let mut session = session();
    write_log(&mut session, LOG, log_line("WARN", "first problem").as_bytes());
    session.pump().expect("session settles");
    append_log(&mut session, LOG, &log_line("WARN", "second problem"));
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert_eq!(count(&panel, "first problem"), 1, "{panel}");
    assert_eq!(count(&panel, "second problem"), 1, "{panel}");
}

/// REQ: COR-050
#[test]
fn cor_050_lines_stay_in_log_order_oldest_first() {
    let mut session = session();
    let text = [log_line("WARN", "alpha"), log_line("ERROR", "beta"), log_line("WARN", "gamma")].concat();
    write_log(&mut session, LOG, text.as_bytes());
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    let alpha = panel.find("alpha").expect("alpha");
    let beta = panel.find("beta").expect("beta");
    let gamma = panel.find("gamma").expect("gamma");
    assert!(alpha < beta && beta < gamma, "{panel}");
}

/// REQ: COR-050
#[test]
fn cor_050_exactly_200_warning_lines_are_all_kept() {
    let mut session = session();
    let text: String = (0..200).map(|index| log_line("WARN", &format!("problem number {index:03}"))).collect();
    write_log(&mut session, LOG, text.as_bytes());
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 400);
    assert!(panel.contains("problem number 000"), "first line dropped");
    assert!(panel.contains("problem number 199"));
}

/// REQ: COR-050
#[test]
fn cor_050_the_201st_warning_line_drops_the_oldest() {
    let mut session = session();
    let text: String = (0..201).map(|index| log_line("WARN", &format!("problem number {index:03}"))).collect();
    write_log(&mut session, LOG, text.as_bytes());
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 400);
    assert!(!panel.contains("problem number 000"), "oldest line kept");
    assert!(panel.contains("problem number 001"));
    assert!(panel.contains("problem number 200"));
}

/// REQ: COR-050
#[test]
fn cor_050_a_log_that_outgrew_one_chunk_adds_the_skipped_marker() {
    let mut session = session();
    let filler: String = (0..2500).map(|index| log_line("WARN", &format!("filler line {index:05} with enough text to fill a chunk"))).collect();
    assert!(filler.len() > 64 * 1024);
    write_log(&mut session, LOG, filler.as_bytes());
    session.pump().expect("session settles");
    assert!(warnings_panel(&mut session, 120, 400).contains("(earlier lines were skipped)"));
}

/// REQ: COR-050
#[test]
fn cor_050_a_small_log_has_no_skipped_marker() {
    let mut session = session();
    write_log(&mut session, LOG, log_line("WARN", "one line").as_bytes());
    session.pump().expect("session settles");
    assert!(!warnings_panel(&mut session, 100, 20).contains("(earlier lines were skipped)"));
}

/// REQ: COR-050
#[test]
fn cor_050_only_the_newest_log_file_of_the_folder_is_tailed() {
    let mut session = session();
    write_log(&mut session, "log-2026-10-11 08-00-00.txt", log_line("WARN", "from the old file").as_bytes());
    write_log(&mut session, LOG, log_line("WARN", "from the new file").as_bytes());
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert!(panel.contains("from the new file"), "{panel}");
    assert!(!panel.contains("from the old file"), "{panel}");
}

/// REQ: COR-050
#[test]
fn cor_050_an_empty_log_leaves_the_panel_without_warning_lines() {
    let mut session = session();
    write_log(&mut session, LOG, b"");
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert!(!panel.contains("WARN"), "{panel}");
}

/// REQ: COR-050
#[test]
fn cor_050_a_partial_last_line_is_not_shown_twice_after_it_is_completed() {
    let mut session = session();
    write_log(&mut session, LOG, b"2026-10-12 09:14:03 WARN half a li");
    session.pump().expect("session settles");
    append_log(&mut session, LOG, "ne\n");
    session.pump().expect("session settles");
    let panel = warnings_panel(&mut session, 100, 20);
    assert_eq!(count(&panel, "half a li"), 1, "{panel}");
}
