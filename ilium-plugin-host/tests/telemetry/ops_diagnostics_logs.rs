//! Requirements OPS-001..011: diagnostics log files, plugin logs, the diagnostics report,
//! and the procedure for a secret found in a log (ops-diagnostics-and-logs.md).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const PLUGIN: &str = "com.example.logs";
const OTHER: &str = "com.example.other";
/// The size of a 1.2 MiB log file in bytes.
const LOG_1_2_MIB: usize = 1_258_291;
/// The report size limit of the whole report in bytes.
const REPORT_MAX_BYTES: usize = 262_144;

/// Every file ending in `.log` below `root`, sorted.
fn log_files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "log") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Every file below `root` whose name contains `needle`, sorted.
fn files_named_like(root: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(needle))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The text of every file in `paths`, concatenated.
fn concatenated_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| fs::read_to_string(path).unwrap_or_default())
        .collect()
}

/// The stdout lines of one CLI run.
fn stdout_lines(output: &CliOutput) -> Vec<String> {
    output.stdout_raw.lines().map(str::to_owned).collect()
}

/// REQ: OPS-001
#[test]
fn ops_001_keeps_five_newest_session_logs_a() {
    let rig = OpsRig::new();
    for _ in 0..7 {
        drop(rig.start_server("default"));
        rig.advance(Duration::from_secs(1));
    }
    let remaining = log_files_under(&rig.state_dir());
    assert_eq!(remaining.len(), 5, "five diagnostics log files remain after seven starts");
}

/// REQ: OPS-001
#[test]
fn ops_001_remaining_logs_are_the_newest_by_start_b() {
    let rig = OpsRig::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut started: Vec<PathBuf> = Vec::new();
    for _ in 0..7 {
        drop(rig.start_server("default"));
        let now = log_files_under(&rig.state_dir());
        if let Some(new_file) = now.iter().find(|path| !seen.contains(path)) {
            started.push(new_file.clone());
        }
        seen = now;
        rig.advance(Duration::from_secs(1));
    }
    assert_eq!(started.len(), 7, "each server start writes one new log file");
    let mut expected = started[2..].to_vec();
    expected.sort();
    assert_eq!(log_files_under(&rig.state_dir()), expected);
}

/// REQ: OPS-002
#[test]
fn ops_002_file_logging_is_off_by_default_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "info", "startup line");
    assert!(
        log_files_under(&rig.state_dir()).is_empty(),
        "no file log is written while file logging is at its default"
    );
}

/// REQ: OPS-002
#[test]
fn ops_002_switching_off_keeps_files_and_stops_lines_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "logging", "file", "on"]);
    rig.emit_plugin_log(PLUGIN, "info", "line written while on");
    let files = log_files_under(&rig.state_dir());
    assert!(!files.is_empty(), "switching file logging on creates a log file");
    let _ = rig.cli(&["plugin", "logging", "file", "off"]);
    rig.emit_plugin_log(PLUGIN, "info", "line written after off");
    assert!(files.iter().all(|path| path.exists()), "switching off deletes no file");
    assert!(
        !concatenated_text(&files).contains("line written after off"),
        "no line is written to the file after the switch"
    );
}

/// REQ: OPS-003
#[test]
fn ops_003_tail_prints_last_five_of_one_plugin_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    for index in 0..50 {
        rig.emit_plugin_log(PLUGIN, "info", &format!("{PLUGIN} entry {index}"));
        rig.emit_plugin_log(OTHER, "info", &format!("{OTHER} entry {index}"));
    }
    let output = rig.cli(&["plugin", "logs", PLUGIN, "--tail", "5"]);
    let lines = stdout_lines(&output);
    assert_eq!(lines.len(), 5, "--tail 5 prints exactly five lines");
    assert!(lines.iter().all(|line| line.contains(PLUGIN) && !line.contains(OTHER)));
}

/// REQ: OPS-003
#[test]
fn ops_003_plain_logs_are_oldest_first_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    for index in 0..50 {
        rig.emit_plugin_log(PLUGIN, "info", &format!("{PLUGIN} entry {index}"));
    }
    let lines = stdout_lines(&rig.cli(&["plugin", "logs", PLUGIN]));
    assert_eq!(lines.len(), 50);
    assert!(lines.first().is_some_and(|line| line.contains("entry 0")));
    assert!(lines.last().is_some_and(|line| line.contains("entry 49")));
}

/// REQ: OPS-004
#[test]
fn ops_004_unreadable_log_folder_is_reported_a() {
    let rig = OpsRig::new();
    let outside = rig.scratch_dir().join("outside-grant");
    fs::create_dir_all(&outside).expect("create the folder outside the grant");
    // SAFETY-free in edition 2021: the variable is only read by this test's own report run.
    std::env::set_var("ILIUM_DEBUG_LOG_DIR", &outside);
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    std::env::remove_var("ILIUM_DEBUG_LOG_DIR");
    assert!(
        report.contains(&format!("log directory not readable: {}", outside.display())),
        "the report names the unreadable log folder"
    );
}

/// REQ: OPS-004
#[test]
fn ops_004_unreadable_log_folder_lists_no_files_and_no_widened_grant_b() {
    let rig = OpsRig::new();
    let outside = rig.scratch_dir().join("outside-grant-b");
    fs::create_dir_all(&outside).expect("create the folder outside the grant");
    fs::write(outside.join("plugin.log"), "secret-free line\n").expect("write a log file");
    std::env::set_var("ILIUM_DEBUG_LOG_DIR", &outside);
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    std::env::remove_var("ILIUM_DEBUG_LOG_DIR");
    assert!(!report.contains("plugin.log"), "no log file of the folder is listed");
    assert!(
        !rig.grants_json().to_string().contains("ILIUM_DEBUG_LOG_DIR"),
        "the grant store is not widened by hand"
    );
}

/// REQ: OPS-005
#[test]
fn ops_005_report_build_opens_no_connection_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.block_network();
    rig.emit_plugin_log(PLUGIN, "info", "report line");
    let _ = rig.cli(&["plugin", "report"]);
    assert_eq!(rig.network_attempts(), 0, "building a report opens no network connection");
}

/// REQ: OPS-005
#[test]
fn ops_005_log_export_opens_no_connection_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.block_network();
    rig.emit_plugin_log(PLUGIN, "info", "export line");
    let destination = rig.scratch_dir().join("exported.log");
    let _ = rig.cli(&["plugin", "logs", PLUGIN, "--export", destination.to_str().unwrap_or("")]);
    assert_eq!(rig.network_attempts(), 0, "the user's own export sends nothing to the network");
}

/// REQ: OPS-006
#[test]
fn ops_006_attach_flow_shows_name_size_and_warning_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log_raw(PLUGIN, &vec![b'x'; LOG_1_2_MIB]);
    let output = rig.cli_tty(&["plugin", "logs", PLUGIN, "--attach"], &["n"]);
    let text = format!("{}{}", output.stdout_text, output.stderr);
    assert!(text.contains(PLUGIN) || text.contains(".log"), "the attach flow shows the file name");
    assert!(text.contains("prompts and replies"), "the attach flow warns that logs hold prompts and replies");
}

/// REQ: OPS-006
#[test]
fn ops_006_attach_flow_shows_size_in_bytes_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log_raw(PLUGIN, &vec![b'x'; LOG_1_2_MIB]);
    let files = rig.plugin_log_files(PLUGIN);
    let size = files
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .max()
        .unwrap_or(0);
    let output = rig.cli_tty(&["plugin", "logs", PLUGIN, "--attach"], &["n"]);
    let text = format!("{}{}", output.stdout_text, output.stderr);
    assert!(size >= LOG_1_2_MIB as u64, "the fixture log is at least 1.2 MiB");
    assert!(text.contains(&size.to_string()), "the size is shown in bytes");
}

/// REQ: OPS-007
#[test]
fn ops_007_support_copy_removes_private_name_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "info", "Reported by Anna Example about the export");
    let destination = rig.scratch_dir().join("support-copy.log");
    let _ = rig.cli_tty(
        &["plugin", "logs", PLUGIN, "--copy", destination.to_str().unwrap_or("")],
        &["y"],
    );
    let copied = fs::read_to_string(&destination).unwrap_or_default();
    assert!(!copied.contains("Anna Example"), "the reviewed copy no longer holds the name");
}

/// REQ: OPS-007
#[test]
fn ops_007_support_copy_removes_paths_outside_home_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "info", "file /srv/private/customer-list.csv opened");
    let destination = rig.scratch_dir().join("support-copy-paths.log");
    let _ = rig.cli_tty(
        &["plugin", "logs", PLUGIN, "--copy", destination.to_str().unwrap_or("")],
        &["y"],
    );
    let copied = fs::read_to_string(&destination).unwrap_or_default();
    assert!(!copied.contains("/srv/private"), "paths outside the home folder are redacted");
}

/// REQ: OPS-008
#[test]
fn ops_008_silent_section_plugin_warned_by_name_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let section = PackageSpec::new("com.example.section", "1.0.0");
    let path = rig.write_package(&section);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    assert!(report.contains("com.example.section"), "the section names the plugin that supplied it");
    assert!(report.contains("2000 ms"), "a section without a reply in 2000 ms gets a warning line");
}

/// REQ: OPS-008
#[test]
fn ops_008_report_still_built_from_other_sections_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let section = PackageSpec::new("com.example.silent", "1.0.0");
    let path = rig.write_package(&section);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
    let report = rig.diagnostics_text();
    assert!(report.contains("host"), "the host section is still present in the report");
    assert!(report.contains("warning"), "the silent section is reported as a warning");
}

/// REQ: OPS-009
#[test]
fn ops_009_oversized_section_is_cut_with_marker_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log_raw(PLUGIN, &vec![b'a'; 70_000]);
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    assert!(report.contains("[section truncated]"), "the cut section ends with the marker line");
}

/// REQ: OPS-009
#[test]
fn ops_009_report_total_is_bounded_and_names_cut_section_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log_raw(PLUGIN, &vec![b'a'; 70_000]);
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    assert!(report.len() <= REPORT_MAX_BYTES, "the whole report is at most 262144 bytes");
    assert!(
        report.lines().any(|line| line.contains("truncated") && line.contains(PLUGIN)),
        "the report says which section was cut"
    );
}

/// REQ: OPS-010
#[test]
fn ops_010_debug_entries_hidden_by_default_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "debug", "debug-entry-010");
    let lines = stdout_lines(&rig.cli(&["plugin", "logs", PLUGIN]));
    assert!(!lines.iter().any(|line| line.contains("debug-entry-010")), "debug entries are hidden at info");
}

/// REQ: OPS-010
#[test]
fn ops_010_threshold_returns_to_info_after_restart_b() {
    let rig = OpsRig::new();
    let server = rig.start_server("default");
    rig.set_log_min_level("debug");
    rig.emit_plugin_log(PLUGIN, "debug", "debug-raised-010");
    let raised = stdout_lines(&rig.cli(&["plugin", "logs", PLUGIN]));
    assert!(raised.iter().any(|line| line.contains("debug-raised-010")), "a raised threshold shows debug");
    drop(server);
    drop(rig.start_server("default"));
    rig.emit_plugin_log(PLUGIN, "debug", "debug-after-restart-010");
    let after = stdout_lines(&rig.cli(&["plugin", "logs", PLUGIN]));
    assert!(!after.iter().any(|line| line.contains("debug-after-restart-010")));
}

/// REQ: OPS-011
#[test]
fn ops_011_secret_found_disables_plugin_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "info", "token sk-fixture-011-secret leaked");
    let _ = rig.cli(&["plugin", "disable", PLUGIN]);
    let status = rig.cli(&["plugin", "status", PLUGIN]).stdout_raw;
    assert!(status.contains("disabled"), "the plugin is disabled as soon as the secret is found");
}

/// REQ: OPS-011
#[test]
fn ops_011_deleting_the_line_is_not_rotation_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.emit_plugin_log(PLUGIN, "info", "token sk-fixture-011-secret leaked");
    let _ = rig.cli(&["plugin", "disable", PLUGIN]);
    for path in rig.plugin_log_files(PLUGIN) {
        let _ = fs::write(&path, "");
    }
    assert!(
        rig.audit_records("plugin.disabled").is_empty() && rig.plugin_log_files(PLUGIN).is_empty(),
        "deleting the log line leaves no audit of a rotation"
    );
    assert!(
        rig.cli(&["plugin", "status", PLUGIN]).stdout_raw.contains("disabled"),
        "the plugin stays disabled until the secret is rotated at its issuer"
    );
}
