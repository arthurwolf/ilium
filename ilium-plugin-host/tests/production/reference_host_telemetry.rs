//! Manual page `reference-host-telemetry`: redaction of e-mail addresses and paths before a log line
//! is written (HST-129), the ban on personal data in logs and counters (HST-130), the 4 KiB log line
//! cut (HST-131), and the trace file rules (HST-979, HST-980, HST-981).

use ilium_plugin_host::trace_file::{erase_plugin_spans, prune_at_server_start, redact_personal};

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-host-telemetry.md";

/// REQ: HST-129
#[test]
fn hst_129_e_mails_and_outside_paths_are_replaced_by_placeholders() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`[redacted-email]` and `[redacted-path]` before a log line or span attribute is written",
    );
}

/// REQ: HST-129
#[test]
fn hst_129_a_log_line_written_with_log_is_redacted_before_it_is_written() {
    let (line, count) = redact_personal(
        "mail ana@example.org read /etc/passwd and /home/ana/notes.txt",
        "/home/ana",
    );
    assert_eq!(line, "mail [redacted-email] read [redacted-path] and /home/ana/notes.txt");
    assert_eq!(count, 2);
    let (clean, none) = redact_personal("plain text", "/home/ana");
    assert_eq!((clean.as_str(), none), ("plain text", 0));
}

/// REQ: HST-130
#[test]
fn hst_130_the_manual_names_personal_data_that_plugins_must_not_log() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "no names, message text, terminal text, prompts or contents of user files",
    );
}

/// REQ: HST-130
#[test]
fn hst_130_the_redactor_does_not_detect_names_so_plugins_are_the_only_control() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "this rule is the only control for them");
}

/// REQ: HST-131
#[test]
fn hst_131_a_log_line_over_4_kib_keeps_its_first_4_kib_and_ends_with_truncated() {
    let long_line = "a".repeat(4 * 1024 + 1);
    let cut = ilium_plugin_host::telemetry::truncate_message(&long_line);
    assert_eq!(cut, format!("{}[truncated]", "a".repeat(4 * 1024)));
}

/// REQ: HST-131
#[test]
fn hst_131_a_log_line_of_exactly_4_kib_is_not_cut() {
    let line = "a".repeat(4 * 1024);
    assert_eq!(ilium_plugin_host::telemetry::truncate_message(&line), line);
}

/// REQ: HST-979
#[test]
fn hst_979_trace_spans_older_than_7_days_are_deleted_at_server_start() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Spans in `<state>/plugin-trace.jsonl` MUST be deleted once they are more than 7 days old (proposed, owner to confirm).",
    );
    assert_page_states(&page, "The host MUST run this deletion at server start.");
}

/// REQ: HST-979
#[test]
fn hst_979_spans_of_seven_days_or_less_survive_the_server_start_deletion() {
    let day = 86_400_000u64;
    let now = 100 * day;
    let folder = tempfile::tempdir().expect("state folder");
    let path = folder.path().join("plugin-trace.jsonl");
    let lines = [
        format!("{{\"plugin\":\"a\",\"time_ms\":{}}}", now - 8 * day),
        format!("{{\"plugin\":\"a\",\"time_ms\":{}}}", now - 7 * day),
        format!("{{\"plugin\":\"a\",\"time_ms\":{}}}", now - day),
    ];
    std::fs::write(&path, lines.join("\n") + "\n").expect("trace file");
    assert_eq!(prune_at_server_start(&path, now).expect("prune"), 1);
    let kept = std::fs::read_to_string(&path).expect("pruned file");
    assert_eq!(kept.lines().count(), 2, "spans of seven days or less survive");
    assert!(!kept.contains(&format!("{}", now - 8 * day)));
}

/// REQ: HST-980
#[test]
fn hst_980_erase_data_rewrites_the_trace_file_and_reports_the_trace_kind() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`ilium plugin erase-data <plugin-id>` MUST remove every span of that plugin from `<state>/plugin-trace.jsonl`",
    );
    assert_page_states(
        &page,
        "by writing a new file and renaming it over the old one",
    );
    assert_page_states(&page, "MUST report the removed count as the kind `trace`");
}

/// REQ: HST-980
#[test]
fn hst_980_spans_of_other_plugins_stay_in_the_trace_file() {
    let folder = tempfile::tempdir().expect("state folder");
    let path = folder.path().join("plugin-trace.jsonl");
    std::fs::write(
        &path,
        "{\"plugin\":\"a\",\"span_id\":1}\n{\"plugin\":\"b\",\"span_id\":2}\n{\"plugin\":\"a\",\"span_id\":3}\n",
    )
    .expect("trace file");
    assert_eq!(erase_plugin_spans(&path, "a").expect("erase"), 2);
    let kept = std::fs::read_to_string(&path).expect("rewritten file");
    assert_eq!(kept.lines().count(), 1);
    assert!(kept.contains("\"plugin\":\"b\""), "spans of other plugins stay: {kept}");
    assert!(!folder.path().join("plugin-trace.jsonl.partial").exists());
}

/// REQ: HST-981
#[test]
fn hst_981_export_data_writes_the_plugin_spans_as_one_jsonl_file() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`ilium plugin export-data` MUST write the spans of the plugin as one JSON-lines file",
    );
}

/// REQ: HST-981
#[test]
fn hst_981_trace_recordings_are_not_included_in_a_support_bundle() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Trace recordings MUST NOT be included in a support bundle",
    );
}
