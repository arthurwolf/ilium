//! Long-lived watchers (plugin build addition, no COR-041 coverage). The page documents how a
//! watcher's lines are applied but no registration route, so these rows are read from the page
//! (see ../API_GAPS.md).

use crate::common::*;

fn watcher_text() -> String {
    let text = page_text();
    let start = text
        .find("A watcher is a child process")
        .expect("watcher paragraph");
    text[start..].lines().next().unwrap_or_default().to_string()
}

/// REQ: F-060
#[test]
fn doc_f_060_a_watcher_prints_one_probe_format_json_line_per_step() {
    let paragraph = watcher_text();
    assert!(
        paragraph.contains("one JSON line per progress step"),
        "{paragraph}"
    );
    for field in ["job_id", "status", "percent", "message", "details", "error"] {
        assert!(paragraph.contains(&format!("`{field}`")), "{field}");
    }
}

/// REQ: F-060, progress-B07
#[test]
fn doc_f_060_a_watcher_line_is_at_most_64_kib() {
    assert!(watcher_text().contains("at most 64 KiB long"));
}

/// REQ: F-060
#[test]
fn doc_f_060_at_most_ten_lines_per_second_are_applied_and_extra_lines_are_coalesced() {
    let paragraph = watcher_text();
    assert!(paragraph.contains("at most 10 lines per second are applied"));
    assert!(paragraph.contains("extra lines are coalesced to the latest"));
}

/// REQ: F-060, progress-B09
#[test]
fn doc_f_060_an_invalid_watcher_line_counts_as_one_probe_failure() {
    assert!(watcher_text().contains("An invalid line counts as one probe failure."));
}

/// REQ: F-060
#[test]
fn doc_f_060_output_arrives_as_child_output_and_child_exited_settles_the_monitor() {
    let paragraph = watcher_text();
    assert!(paragraph.contains("`child-output`"));
    assert!(paragraph.contains("settles the monitor on `child-exited`"));
}

/// REQ: F-060
#[test]
fn f_060_the_server_side_holds_the_long_lived_process_grant_the_watchers_need() {
    let manifest = manifest_value();
    let server = strings(&manifest["capabilities"]["server"]);
    assert!(server.contains(&"process.long-lived".to_string()));
}
