//! Gap requirements of lane `observability` (OPS-000, OPS-012..018): telemetry consent as a
//! read-only setting, consent reset on a privacy declaration change, the no-declaration refusal,
//! the policy block, restore and import of consent, diagnostics log folder safety and the log size cap.
//! Tests are written before the implementation: they are expected to fail at runtime until it exists.

use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const PLUGIN: &str = "com.example.observed";
/// The diagnostics log size cap in bytes (64 MiB, proposed in OPS-018).
const LOG_CAP_BYTES: u64 = 64 * 1024 * 1024;
/// One log line of a fixed size used to fill the diagnostics log.
const FILL_LINE: &str = "2026-11-04 10:00:00 INFO plugin com.example.observed filler line of fixed length padding\n";

/// Installs the observed fixture plugin through the CLI.
fn install_observed(rig: &OpsRig) {
    let spec = PackageSpec::new(PLUGIN, "1.0.0");
    let path = rig.write_package(&spec);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
}

/// Installs a version of the observed plugin whose `[privacy]` table is the given one.
fn install_observed_with_privacy(rig: &OpsRig, version: &str, collects: &[&str]) {
    let spec = PackageSpec::new(PLUGIN, version).privacy_table(collects, 30, "privacy@example.com");
    let path = rig.write_package(&spec);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
}

/// Every file below `root`, sorted.
fn files_under(root: &Path) -> Vec<PathBuf> {
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
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The numeric user id of the running user, read from a file the test creates itself.
fn running_uid(rig: &OpsRig) -> u32 {
    let probe = rig.scratch_dir().join("uid-probe");
    let _ = fs::write(&probe, b"probe");
    fs::metadata(&probe).map(|metadata| metadata.uid()).unwrap_or(u32::MAX)
}

/// REQ: OPS-000
#[test]
fn req_ops_000_refuses_telemetry_host_without_consent_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed(&rig);
    let declared = rig.cli(&["plugin", "fetch", PLUGIN, "https://telemetry.example.com/collect"]);
    let ordinary = rig.cli(&["plugin", "fetch", PLUGIN, "https://api.example.org/ordinary"]);
    assert!(
        declared.stderr.contains("not-permitted") || declared.exit_code != 0,
        "a telemetry host is refused while consent is not-asked"
    );
    assert!(
        ordinary.exit_code == 0,
        "a host outside telemetry_hosts is not refused on the telemetry ground"
    );
}

/// REQ: OPS-000
#[test]
fn req_ops_000_refusal_is_logged_and_lifts_on_grant_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed(&rig);
    let _ = rig.cli(&["plugin", "fetch", PLUGIN, "https://telemetry.example.com/collect"]);
    let log = rig.cli(&["plugin", "logs", PLUGIN]).stdout_raw;
    assert!(
        log.contains("refused telemetry:telemetry.example.com"),
        "the refusal writes the log line refused telemetry:<host>"
    );
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let after = rig.cli(&["plugin", "fetch", PLUGIN, "https://telemetry.example.com/collect"]);
    assert_eq!(after.exit_code, 0, "with consent granted the telemetry host request succeeds");
}

/// REQ: OPS-012
#[test]
fn req_ops_012_consent_setting_reads_each_state_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed(&rig);
    let not_asked = rig.cli(&["plugin", "settings", PLUGIN, "--get", "telemetry.consent"]).stdout_raw;
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let granted = rig.cli(&["plugin", "settings", PLUGIN, "--get", "telemetry.consent"]).stdout_raw;
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "decline"]);
    let declined = rig.cli(&["plugin", "settings", PLUGIN, "--get", "telemetry.consent"]).stdout_raw;
    assert!(not_asked.contains("not-asked"), "the fresh state reads not-asked");
    assert!(granted.contains("granted"), "the granted state reads granted");
    assert!(declined.contains("declined"), "the declined state reads declined");
}

/// REQ: OPS-012
#[test]
fn req_ops_012_consent_change_emits_settings_changed_and_refuses_write_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed(&rig);
    let change = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]).stdout_raw;
    assert!(
        change.contains("settings-changed") && change.contains("telemetry.consent"),
        "one settings-changed event naming telemetry.consent arrives after the change"
    );
    let write = rig.cli(&["plugin", "settings", PLUGIN, "--set", "telemetry.consent=granted"]);
    assert!(write.stderr.contains("not-permitted"), "a plugin write to telemetry.consent is not-permitted");
}

/// REQ: OPS-013
#[test]
fn req_ops_013_privacy_change_resets_granted_consent_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    install_observed_with_privacy(&rig, "2.0.0", &["usage-counts", "crash-reports"]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "a changed privacy table resets granted consent to not-asked");
    let withdrawn = rig.audit_records("consent-withdrawn");
    let with_reason = withdrawn
        .iter()
        .filter(|record| record.to_string().contains("declaration-changed"))
        .count();
    assert_eq!(with_reason, 1, "exactly one consent-withdrawn record names declaration-changed");
}

/// REQ: OPS-013
#[test]
fn req_ops_013_unchanged_privacy_table_keeps_granted_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    install_observed_with_privacy(&rig, "1.0.1", &["usage-counts"]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("granted"), "an unchanged privacy table keeps the state granted");
    assert!(
        !rig.cli(&["plugin", "settings", PLUGIN]).stdout_raw.contains("privacy declaration changed"),
        "the settings page shows no privacy change notice when the table is unchanged"
    );
}

/// REQ: OPS-014
#[test]
fn req_ops_014_no_privacy_table_disables_grant_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let spec = PackageSpec::new(PLUGIN, "1.0.0");
    let path = rig.write_package(&spec);
    let _ = rig.cli(&["plugin", "install", path.to_str().unwrap_or("")]);
    let settings = rig.cli(&["plugin", "settings", PLUGIN]).stdout_raw;
    assert!(
        settings.contains("This plugin does not describe what it collects"),
        "the settings page explains that the plugin has no privacy declaration"
    );
    let grant = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    assert!(grant.exit_code != 0, "the CLI grant fails without a privacy declaration");
    assert!(
        grant.stderr.contains("consent.no-privacy-declaration"),
        "the CLI refuses with consent.no-privacy-declaration"
    );
}

/// REQ: OPS-014
#[test]
fn req_ops_014_state_stays_not_asked_without_declaration_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed(&rig);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "the state remains not-asked when the grant is refused");
    assert!(!grants.contains("granted"), "no granted state is recorded for an undeclared plugin");
}

/// REQ: OPS-015
#[test]
fn req_ops_015_policy_block_shown_and_state_kept_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.write_policy("[telemetry]\nplugins = \"off\"\n");
    rig.reload_policy();
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let settings = rig.cli(&["plugin", "settings", PLUGIN]).stdout_raw;
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(settings.contains("granted") && settings.contains("blocked by policy"));
    assert!(grants.contains("granted") && grants.contains("blocked by policy"));
}

/// REQ: OPS-015
#[test]
fn req_ops_015_policy_removal_clears_block_and_keeps_grant_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    rig.write_policy("[telemetry]\nplugins = \"off\"\n");
    rig.reload_policy();
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    rig.write_policy("");
    drop(_server);
    let _restarted = rig.start_server("default");
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(!grants.contains("blocked by policy"), "removing the policy clears the block label");
    assert!(grants.contains("granted"), "the user's own granted state is kept after the restart");
}

/// REQ: OPS-016
#[test]
fn req_ops_016_restore_without_flag_resets_telemetry_to_not_asked_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let backup = rig.scratch_dir().join("backup-observed.json");
    let _ = rig.cli(&["plugin", "backup", "--output", backup.to_str().unwrap_or("")]);
    let _ = rig.cli(&["plugin", "erase", PLUGIN, "--yes"]);
    let _ = rig.cli(&["plugin", "restore", "--include-grants", backup.to_str().unwrap_or("")]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("not asked"), "restore without --include-consents restores telemetry as not-asked");
    assert!(
        rig.audit_records("consent-given").is_empty(),
        "no consent record is written when the consent was not restored"
    );
}

/// REQ: OPS-016
#[test]
fn req_ops_016_restore_with_include_consents_writes_audit_record_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    install_observed_with_privacy(&rig, "1.0.0", &["usage-counts"]);
    let _ = rig.cli(&["plugin", "telemetry", PLUGIN, "grant"]);
    let backup = rig.scratch_dir().join("backup-observed-consents.json");
    let _ = rig.cli(&["plugin", "backup", "--output", backup.to_str().unwrap_or("")]);
    let _ = rig.cli(&["plugin", "erase", PLUGIN, "--yes"]);
    let _ = rig.cli(&[
        "plugin",
        "restore",
        "--include-grants",
        "--include-consents",
        backup.to_str().unwrap_or(""),
    ]);
    let grants = rig.cli(&["plugin", "grants", PLUGIN]).stdout_raw;
    assert!(grants.contains("granted"), "restore with --include-consents restores granted");
    let records = rig.audit_records("consent-given");
    let by_restore = records
        .iter()
        .filter(|record| record.to_string().contains("restore"))
        .count();
    assert_eq!(by_restore, 1, "one consent-given record names the actor restore");
}

/// REQ: OPS-017
#[test]
fn req_ops_017_refuses_log_folder_that_is_a_link_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let uid = running_uid(&rig);
    let root = Path::new("/tmp").join(format!(".ilium-{uid}"));
    assert!(!root.exists(), "the test needs /tmp/.ilium-<uid> to be absent before it runs");
    let scratch_target = rig.scratch_dir().join("link-target");
    fs::create_dir_all(&scratch_target).unwrap_or(());
    let _ = symlink(&scratch_target, &root);
    let output = rig.cli(&["plugin", "logging", "file", "on"]);
    let _ = fs::remove_file(&root);
    assert!(
        output.stderr.contains("log directory not writable"),
        "a link at the log root is refused with the notice log directory not writable"
    );
    assert!(
        files_under(&scratch_target).is_empty(),
        "no file is written through the symbolic link"
    );
}

/// REQ: OPS-017
#[test]
fn req_ops_017_created_folders_and_files_have_private_modes_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let uid = running_uid(&rig);
    let root = Path::new("/tmp").join(format!(".ilium-{uid}"));
    assert!(!root.exists(), "the test needs /tmp/.ilium-<uid> to be absent before it runs");
    let _ = rig.cli(&["plugin", "logging", "file", "on"]);
    let root_mode = fs::metadata(&root).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
    assert_eq!(root_mode, 0o700, "the root folder is created with mode 0700");
    let session_logs = root.join("logs");
    let session_mode = fs::metadata(&session_logs).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
    assert_eq!(session_mode, 0o700, "the logs folder is created with mode 0700");
    let log_file_mode = files_under(&session_logs)
        .into_iter()
        .map(|path| fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0))
        .collect::<Vec<u32>>();
    assert!(
        !log_file_mode.is_empty() && log_file_mode.iter().all(|mode| *mode == 0o600),
        "each log file is created with mode 0600"
    );
    let _ = fs::remove_dir_all(&root);
}

/// REQ: OPS-018
#[test]
fn req_ops_018_log_stops_at_cap_with_marker_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "logging", "file", "on"]);
    let log_path = rig
        .state_dir()
        .join("plugins")
        .join("logs")
        .join(format!("{PLUGIN}.log"));
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|error| panic!("open {}: {error}", log_path.display()));
    let line_bytes = FILL_LINE.len() as u64;
    let lines_needed = LOG_CAP_BYTES / line_bytes + 1;
    for _ in 0..lines_needed {
        let _ = file.write_all(FILL_LINE.as_bytes());
    }
    let _ = rig.cli(&["plugin", "report"]);
    let text = fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        text.trim_end().ends_with("[log cap reached, further lines dropped]"),
        "the file ends with the cap marker line"
    );
}

/// REQ: OPS-018
#[test]
fn req_ops_018_dropped_count_shown_in_report_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "logging", "file", "on"]);
    let log_path = rig
        .state_dir()
        .join("plugins")
        .join("logs")
        .join(format!("{PLUGIN}.log"));
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&log_path, vec![b'x'; LOG_CAP_BYTES as usize]);
    for _ in 0..1000 {
        let _ = rig.cli(&["plugin", "logs", PLUGIN, "--tail", "1"]);
    }
    let report = rig.cli(&["plugin", "report"]).stdout_raw;
    let size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(u64::MAX);
    assert!(
        report.contains("dropped") && report.contains("1000"),
        "the diagnostics report shows the count of dropped lines"
    );
    assert!(size <= LOG_CAP_BYTES + FILL_LINE.len() as u64, "the file stays within the cap plus one line");
}
