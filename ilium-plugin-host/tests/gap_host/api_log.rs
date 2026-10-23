//! Manual page `api-log`: rotation, retention, JSON lines, sanitising and the level filter of the
//! plugin log (HST-600, 601, 602, 604, 605).

use super::common::{install_ok, pkg, rig, NOTES};

const ROTATE_BYTES: u64 = 4_194_304;

/// REQ: HST-600
#[test]
fn hst_600_rotates_one_log_file_per_plugin_at_4_mib_keeping_three() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log_bytes(NOTES, 17 * ROTATE_BYTES);
    let files = rig.plugin_log_files(NOTES);
    let active_files: Vec<_> = files
        .iter()
        .filter(|path| path.ends_with(format!("{NOTES}.log")))
        .collect();
    assert_eq!(active_files.len(), 1, "one active log file per plugin id");
    let rotated = files.len() - 1;
    assert_eq!(rotated, 3, "at most three rotated files per plugin");
    for path in &files {
        let size = std::fs::metadata(path).expect("log file exists").len();
        assert!(
            size <= ROTATE_BYTES,
            "{} exceeds 4 MiB: {size}",
            path.display()
        );
    }
}

/// REQ: HST-601
#[test]
fn hst_601_deletes_rotated_logs_older_than_14_days_at_start_and_on_purge() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_plugin_log_age_days(NOTES, 15);
    let server = rig.start_server("default");
    assert!(
        rig.plugin_rotated_logs(NOTES).is_empty(),
        "rotated logs older than 14 days removed at start"
    );
    rig.emit_plugin_log_bytes(NOTES, ROTATE_BYTES);
    let output = rig.cli(&["uninstall", NOTES, "--purge", "--yes"]);
    assert_eq!(output.exit_code, 0, "purge uninstall succeeds");
    assert!(
        rig.plugin_log_files(NOTES).is_empty(),
        "log files gone within 1 minute of uninstall with purge"
    );
    server.shutdown();
}

/// REQ: HST-602
#[test]
fn hst_602_log_lines_are_json_objects_with_escaped_newlines() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log(NOTES, "info", "first line\nsecond line");
    let output = rig.cli(&["plugin", "logs", NOTES, "--json"]);
    assert_eq!(output.exit_code, 0, "logs command succeeds");
    let line = output.stdout_jsonl.last().expect("one log line").clone();
    for field in [
        "ts", "level", "plugin", "side", "instance", "code", "message",
    ] {
        assert!(line.get(field).is_some(), "log line carries field {field}");
    }
    let timestamp = line["ts"].as_str().expect("ts is a string");
    assert!(
        timestamp.ends_with('Z') && timestamp.contains('.'),
        "RFC 3339 UTC with milliseconds: {timestamp}"
    );
    assert!(
        !output
            .stdout_raw
            .lines()
            .any(|raw| raw.contains("first line") && !raw.contains("\\n")),
        "no raw newline inside message"
    );
}

/// REQ: HST-604
#[test]
fn hst_604_invalid_utf8_and_nul_are_replaced_and_counted() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.emit_plugin_log_raw(NOTES, b"bad \xff byte and nul \x00 here");
    let output = rig.cli(&["plugin", "logs", NOTES, "--json"]);
    let message = output.stdout_jsonl.last().expect("log line accepted")["message"]
        .as_str()
        .expect("message is a string")
        .to_owned();
    assert!(
        message.contains('\u{FFFD}'),
        "invalid bytes replaced by U+FFFD"
    );
    assert!(message.contains('\u{2400}'), "NUL replaced by U+2400");
    assert_eq!(
        rig.plugin_side_counter(NOTES, "log_sanitized"),
        1,
        "event counted in log_sanitized"
    );
}

/// REQ: HST-605
#[test]
fn hst_605_level_filter_runs_before_the_rate_limit() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_log_min_level("info");
    rig.emit_plugin_log_lines(NOTES, "debug", 1_000);
    assert_eq!(
        rig.plugin_rate_budget_used(NOTES),
        0,
        "debug lines below info do not use the 200-per-second budget"
    );
    assert!(
        rig.plugin_log_files(NOTES)
            .iter()
            .all(|path| !std::fs::read_to_string(path)
                .unwrap_or_default()
                .contains("debug")),
        "debug lines not stored"
    );
}
