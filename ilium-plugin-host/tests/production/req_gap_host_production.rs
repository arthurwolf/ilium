//! Gap requirements of lane `host-production` (HST-136..142): bundle exclusions of telemetry kinds,
//! system-wide installs and their integrity checks, the root inventory, log sanitizing, trace file
//! modes and counter reset on uninstall.
//! Tests are written before the implementation: they are expected to fail at runtime until it exists.

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

const PLUGIN: &str = "com.example.production";
const OTHER: &str = "com.example.production-two";
/// Plugin-supplied text with an OSC escape, a C1 CSI and a BEL, as HST-140 names them.
const HOSTILE_LOG: &str = "start \u{1b}]0;title\u{7} then \u{9b}31m red";
const TRACE_VARIABLE: &str = "ILIUM_PLUGIN_TRACE";
/// The four kinds HST-136 requires to be excluded, as they must be named in `summary.txt`.
const EXCLUDED_KINDS: [&str; 4] = [
    "inference privacy log",
    "trace span",
    "consent record",
    "daily summary",
];
/// File-name fragments that must not appear in any bundle entry name.
const EXCLUDED_NAME_MARKERS: [&str; 4] = ["inference", "trace", "consent", "summar"];

/// Installs a plugin for the current user through the CLI.
fn install_plugin(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["plugin", "install", &path_text, "--yes", "--allow-unsigned"])
}

/// Installs a plugin system-wide through the CLI.
fn install_system(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["plugin", "install", "--system", &path_text, "--yes", "--allow-unsigned"])
}

/// A bundle output path inside the rig's scratch folder.
fn bundle_path(rig: &OpsRig, name: &str) -> PathBuf {
    rig.scratch_dir().join(name)
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

/// The entries of a zip bundle as (name, text) pairs, or `None` when the file is not a readable zip.
fn read_bundle(bundle: &Path) -> Option<Vec<(String, String)>> {
    let file = fs::File::open(bundle).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).ok()?;
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).ok()?;
        entries.push((name, String::from_utf8_lossy(&bytes).into_owned()));
    }
    Some(entries)
}

/// The text of the `summary.txt` entry of a bundle.
fn summary_text(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .find(|(name, _)| name.ends_with("summary.txt"))
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

/// The permission bits (mode & 0o777) of a path, if it exists.
fn mode_of(path: &Path) -> Option<u32> {
    fs::metadata(path).ok().map(|metadata| metadata.permissions().mode() & 0o777)
}

/// True when every number reachable in the JSON value is zero.
fn all_numbers_zero(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n == 0.0),
        serde_json::Value::Object(map) => map.values().all(all_numbers_zero),
        serde_json::Value::Array(items) => items.iter().all(all_numbers_zero),
        _ => true,
    }
}

/// REQ: HST-136
#[test]
fn req_hst_136_bundles_exclude_telemetry_kinds_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let diagnostics = bundle_path(&rig, "diagnostics.zip");
    let support = bundle_path(&rig, "support.zip");
    let diagnostics_path = diagnostics.to_string_lossy().into_owned();
    let support_path = support.to_string_lossy().into_owned();
    let _ = rig.cli(&["plugin", "diagnostics", "--out", &diagnostics_path]);
    let _ = rig.cli(&["plugin", "support-bundle", PLUGIN, "--out", &support_path]);
    for (label, bundle) in [("diagnostics", &diagnostics), ("support", &support)] {
        let entries = read_bundle(bundle)
            .unwrap_or_else(|| panic!("the {label} bundle must be written as a readable zip"));
        for (name, _) in &entries {
            for marker in EXCLUDED_NAME_MARKERS {
                assert!(
                    !name.contains(marker),
                    "the {label} bundle must not contain an entry for {marker}, found {name}"
                );
            }
        }
    }
}

/// REQ: HST-136
#[test]
fn req_hst_136_bundles_name_four_excluded_kinds_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let diagnostics = bundle_path(&rig, "diagnostics-summary.zip");
    let support = bundle_path(&rig, "support-summary.zip");
    let diagnostics_path = diagnostics.to_string_lossy().into_owned();
    let support_path = support.to_string_lossy().into_owned();
    let _ = rig.cli(&["plugin", "diagnostics", "--out", &diagnostics_path]);
    let _ = rig.cli(&["plugin", "support-bundle", PLUGIN, "--out", &support_path]);
    for (label, bundle) in [("diagnostics", &diagnostics), ("support", &support)] {
        let entries = read_bundle(bundle)
            .unwrap_or_else(|| panic!("the {label} bundle must be written as a readable zip"));
        let summary = summary_text(&entries);
        let excluded_lines: Vec<&str> = summary
            .lines()
            .filter(|line| line.starts_with("excluded:"))
            .collect();
        assert!(
            excluded_lines.len() == 4,
            "the {label} summary.txt must hold exactly four `excluded:` lines, found {}",
            excluded_lines.len()
        );
        for kind in EXCLUDED_KINDS {
            assert!(
                excluded_lines.iter().any(|line| line.contains(kind)),
                "the {label} summary.txt must name {kind} as excluded"
            );
        }
    }
}

/// REQ: HST-137
#[test]
fn req_hst_137_system_install_writes_system_package_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let run = install_system(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    assert!(
        run.exit_code == 0,
        "a system install run with the required privilege must succeed, got exit {} and stderr: {}",
        run.exit_code,
        run.stderr
    );
    let user_registry = rig.registry_json().to_string();
    assert!(
        !user_registry.contains(PLUGIN),
        "a system install must not change any user's installed.json"
    );
    assert!(
        run.stdout_text.contains("system") || run.stderr.contains("system"),
        "the system install must report that it wrote the system package directory"
    );
}

/// REQ: HST-137
#[test]
fn req_hst_137_system_install_refused_without_admin_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let run = install_system(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    assert!(
        run.exit_code != 0,
        "a system install run without the required privilege must be refused"
    );
    assert!(
        run.stderr.contains("install.system-requires-admin"),
        "the refusal must name install.system-requires-admin, got: {}",
        run.stderr
    );
    assert!(
        !rig.registry_json().to_string().contains(PLUGIN),
        "a refused system install must write nothing into the user's registry"
    );
}

/// REQ: HST-138
#[test]
fn req_hst_138_integrity_failure_not_loaded_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_system(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let component = files_under(&rig.scratch_dir())
        .into_iter()
        .filter(|path| path.to_string_lossy().contains("system"))
        .filter(|path| fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false))
        .next_back()
        .expect("the system install must leave a package file under the scratch root");
    let mut bytes = fs::read(&component).expect("the system package file must be readable");
    if let Some(last) = bytes.last_mut() {
        *last ^= 0x01;
    }
    fs::write(&component, &bytes).expect("the test must be able to alter one byte");
    let _restarted = rig.start_server("default");
    let doctor = rig.cli(&["plugin", "doctor"]);
    assert!(
        doctor.stderr.contains("integrity-failure") && doctor.stderr.contains(PLUGIN),
        "doctor must report integrity-failure for {PLUGIN}, got: {}",
        doctor.stderr
    );
    assert!(
        doctor.stderr.contains(&component.to_string_lossy().into_owned()),
        "doctor must name the system path of the failing package"
    );
}

/// REQ: HST-138
#[test]
fn req_hst_138_user_copy_does_not_repair_system_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(OTHER, "1.0.0"));
    let _ = install_system(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let doctor = rig.cli(&["plugin", "doctor"]);
    assert!(
        !doctor.stdout_text.contains("repaired") && !doctor.stderr.contains("repaired"),
        "a user copy of the same id must not repair a failing system package"
    );
    let audit = rig.cli(&["plugin", "audit", PLUGIN]);
    assert!(
        audit.stdout_text.contains("integrity-failure") || audit.stderr.contains("integrity-failure"),
        "the affected user's audit log must hold integrity-failure for {PLUGIN}"
    );
}

/// REQ: HST-139
#[test]
fn req_hst_139_root_inventory_lists_every_user_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let _ = install_plugin(&rig, &PackageSpec::new(OTHER, "1.0.0"));
    let run = rig.cli(&["plugin", "admin", "inventory", "--json"]);
    assert!(
        run.exit_code == 0,
        "admin inventory must succeed, got exit {} and stderr: {}",
        run.exit_code,
        run.stderr
    );
    let rows: Vec<serde_json::Value> = run
        .stdout_raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or(serde_json::Value::Null))
        .collect();
    assert!(
        rows.len() == 2,
        "inventory must print one line per installed plugin, found {} lines",
        rows.len()
    );
    for row in &rows {
        assert!(
            row.get("user").is_some() && row.get("id").is_some(),
            "every inventory line must carry the user and id fields, got {row}"
        );
    }
    let keys: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row["user"].as_str().unwrap_or_default().to_owned(),
                row["id"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert!(keys == sorted, "inventory lines must be sorted by user then id");
}

/// REQ: HST-139
#[test]
fn req_hst_139_running_session_is_read_not_refused_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let run = rig.cli(&["plugin", "admin", "inventory", "--json"]);
    assert!(
        run.exit_code == 0 && !run.stderr.contains("refus"),
        "a user with a running session server must be read by inventory, not refused; stderr: {}",
        run.stderr
    );
    assert!(
        run.stdout_raw.contains(PLUGIN),
        "the inventory must list the plugin of the user with a running server"
    );
}

/// REQ: HST-140
#[test]
fn req_hst_140_control_characters_replaced_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0").guest_message(HOSTILE_LOG));
    let logs = rig.cli(&["plugin", "logs", PLUGIN]);
    let text = &logs.stdout_text;
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}') && !text.contains('\u{9b}'),
        "logs must not print ESC, BEL or the C1 CSI from plugin text"
    );
    assert!(
        text.matches('\u{FFFD}').count() >= 2,
        "each escape run must become U+FFFD, got: {text:?}"
    );
    let stats = rig.cli(&["plugin", "stats", PLUGIN, "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(stats.stdout_raw.trim()).unwrap_or_default();
    assert!(
        parsed["log_sanitized"].as_u64().unwrap_or(0) > 0,
        "stats must count the replacements under log_sanitized"
    );
    let json_logs = rig.cli(&["plugin", "logs", PLUGIN, "--json"]);
    assert!(
        json_logs.stdout_raw.contains("\\u001b"),
        "--json output must escape ESC as \\u001b"
    );
}

/// REQ: HST-140
#[test]
fn req_hst_140_tab_is_kept_and_not_counted_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(
        &rig,
        &PackageSpec::new(PLUGIN, "1.0.0").guest_message("column one\tcolumn two"),
    );
    let logs = rig.cli(&["plugin", "logs", PLUGIN]);
    assert!(
        logs.stdout_text.contains('\t'),
        "a tab in plugin text is not a control character to replace and must be printed"
    );
    let stats = rig.cli(&["plugin", "stats", PLUGIN, "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(stats.stdout_raw.trim()).unwrap_or_default();
    assert!(
        parsed["log_sanitized"].as_u64() == Some(0),
        "a tab must not increment log_sanitized"
    );
}

/// REQ: HST-141
#[test]
fn req_hst_141_trace_file_modes_when_enabled_a() {
    std::env::set_var(TRACE_VARIABLE, "1");
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let trace = rig.state_dir().join("plugin-trace.jsonl");
    let file_mode = mode_of(&trace);
    let folder_mode = mode_of(&rig.state_dir());
    std::env::remove_var(TRACE_VARIABLE);
    assert!(
        file_mode == Some(0o600),
        "the trace file must exist with mode 0600, got {file_mode:?}"
    );
    assert!(
        folder_mode == Some(0o700),
        "the folder holding the trace must be 0700, got {folder_mode:?}"
    );
}

/// REQ: HST-141
#[test]
fn req_hst_141_no_trace_file_without_variable_b() {
    std::env::remove_var(TRACE_VARIABLE);
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let trace = rig.state_dir().join("plugin-trace.jsonl");
    assert!(
        !trace.exists(),
        "no trace file may be created while {TRACE_VARIABLE} is unset"
    );
}

/// REQ: HST-142
#[test]
fn req_hst_142_reinstall_starts_counters_at_zero_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let _ = rig.cli(&["plugin", "uninstall", PLUGIN, "--yes"]);
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let stats = rig.cli(&["plugin", "stats", PLUGIN, "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(stats.stdout_raw.trim())
        .unwrap_or(serde_json::Value::Null);
    assert!(
        !parsed.is_null() && all_numbers_zero(&parsed),
        "after uninstall and reinstall every counter must be zero, got {}",
        stats.stdout_raw
    );
}

/// REQ: HST-142
#[test]
fn req_hst_142_reinstall_has_no_daily_summaries_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let _ = rig.cli(&["plugin", "uninstall", PLUGIN, "--yes"]);
    let _ = install_plugin(&rig, &PackageSpec::new(PLUGIN, "1.0.0"));
    let status = rig.cli(&["plugin", "status", PLUGIN, "--days", "30"]);
    assert!(
        status.exit_code == 0,
        "status must run for the reinstalled plugin, got exit {}",
        status.exit_code
    );
    assert!(
        status.stdout_text.contains("failures in window: 0"),
        "the failure window must be empty after reinstall, got: {}",
        status.stdout_text
    );
    let daily_rows = status
        .stdout_text
        .lines()
        .filter(|line| line.trim_start().starts_with("20"))
        .count();
    assert!(
        daily_rows == 0,
        "no daily summary of the earlier install may remain, found {daily_rows} rows"
    );
}
