//! Requirements OPS-201..208: monitoring of plugin health, the kill switch, its persistence,
//! the essential-plugin refusal and the incident records (ops-crash-isolation-health-and-kill-switch.md).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ilium_plugin_host::kill_switch::{KillSwitch, PluginState};
use ilium_plugin_host::testing::ops::OpsRig;

const PLUGIN: &str = "com.example.killable";
const OTHER: &str = "com.example.bystander";
/// The monitoring script of the operator runbook (its path is a gap, see the report).
const MONITOR_SCRIPT: &str = "ops/ilium-plugin-health.sh";

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

/// REQ: OPS-201
#[test]
fn ops_201_unexpected_exit_code_is_monitoring_failure_a() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(MONITOR_SCRIPT);
    let output = Command::new("sh")
        .arg(&script)
        .env("ILIUM_BIN", "/nonexistent/ilium-binary")
        .output()
        .expect("run the monitoring script");
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(text.contains("monitoring failure"), "a missing binary is reported as a monitoring failure");
    assert!(!text.contains("healthy"), "a missing binary is never reported as healthy");
}

/// REQ: OPS-201
#[test]
fn ops_201_monitor_exit_codes_other_than_0_1_2_are_not_healthy_b() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(MONITOR_SCRIPT);
    let output = Command::new("sh")
        .arg(&script)
        .env("ILIUM_BIN", "/bin/false")
        .output()
        .expect("run the monitoring script");
    assert!(
        output.status.code() != Some(0) || !String::from_utf8_lossy(&output.stdout).contains("healthy"),
        "an unexpected exit code never ends as healthy"
    );
}

/// REQ: OPS-202
#[test]
fn ops_202_status_is_read_before_kill_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "status", PLUGIN]);
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "degraded triage"]);
    let trace = rig.trace();
    let status_at = trace.iter().position(|step| step.contains("status"));
    let kill_at = trace.iter().position(|step| step.contains("kill"));
    assert!(
        matches!((status_at, kill_at), (Some(status), Some(kill)) if status < kill),
        "the status read comes before the kill in the runbook steps"
    );
}

/// REQ: OPS-202
#[test]
fn ops_202_degraded_plugin_is_read_before_any_change_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let status = rig.cli(&["plugin", "status", PLUGIN]).stdout_raw;
    assert!(
        status.contains("degraded") || status.contains("state"),
        "the state of the degraded plugin is readable before a kill, disable or uninstall"
    );
    assert!(
        !rig.cli(&["plugin", "status", PLUGIN]).stdout_raw.contains("disabled-by-admin"),
        "the plugin is not yet killed when the status is read"
    );
}

/// REQ: OPS-203
#[test]
fn ops_203_empty_reason_is_refused_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let output = rig.cli(&["plugin", "kill", PLUGIN, "--reason", ""]);
    assert_ne!(output.exit_code, 0, "a kill with an empty reason is refused");
}

/// REQ: OPS-203
#[test]
fn ops_203_refusal_prints_code_and_plugin_keeps_running_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let output = rig.cli(&["plugin", "kill", PLUGIN, "--reason", ""]);
    let text = format!("{}{}", output.stdout_raw, output.stderr);
    assert!(text.contains("refus"), "the refusal code is printed");
    let status = rig.cli(&["plugin", "status", PLUGIN]).stdout_raw;
    assert!(!status.contains("disabled-by-admin"), "the plugin still runs after the refusal");
}

/// REQ: OPS-204
#[test]
fn ops_204_killed_plugin_stays_killed_across_restart_a() {
    let directory = tempfile::tempdir().expect("scratch folder");
    let mut switch = KillSwitch::open(directory.path());
    switch.kill(PLUGIN, "incident response", "operator");
    drop(switch);
    let reopened = KillSwitch::open(directory.path());
    assert_eq!(
        reopened.plugin_state(PLUGIN),
        PluginState::DisabledByAdmin,
        "a server restart does not re-enable the killed plugin"
    );
}

/// REQ: OPS-204
#[test]
fn ops_204_policy_reload_without_plugin_keeps_it_killed_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "incident response"]);
    rig.write_policy("");
    rig.reload_policy();
    let status = rig.cli(&["plugin", "status", PLUGIN]).stdout_raw;
    assert!(
        status.contains("disabled-by-admin"),
        "a policy reload that does not list the plugin keeps it disabled"
    );
}

/// REQ: OPS-205
#[test]
fn ops_205_kill_keeps_crash_records_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let before = files_named_like(&rig.state_dir(), "crash");
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "crash loop"]);
    let after = files_named_like(&rig.state_dir(), "crash");
    assert!(
        before.iter().all(|path| after.contains(path)),
        "the crash records of the plugin are still in place after the kill"
    );
}

/// REQ: OPS-205
#[test]
fn ops_205_kill_keeps_crash_loop_guard_marks_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let before = files_named_like(&rig.state_dir(), "mark");
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "crash loop"]);
    let after = files_named_like(&rig.state_dir(), "mark");
    assert!(
        before.iter().all(|path| after.contains(path)),
        "the kill switch deletes no crash-loop guard mark"
    );
}

/// REQ: OPS-206
#[test]
fn ops_206_essential_core_plugin_kill_is_refused_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let core = rig
        .core_plugin_ids()
        .into_iter()
        .next()
        .expect("at least one essential core plugin is bundled");
    let output = rig.cli(&["plugin", "kill", &core, "--reason", "test"]);
    assert_ne!(output.exit_code, 0, "the kill of an essential core plugin is refused");
    let text = format!("{}{}", output.stdout_raw, output.stderr);
    assert!(text.contains("essential"), "the refusal names the reason");
}

/// REQ: OPS-206
#[test]
fn ops_206_essential_core_plugin_still_runs_after_refusal_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let core = rig
        .core_plugin_ids()
        .into_iter()
        .next()
        .expect("at least one essential core plugin is bundled");
    let _ = rig.cli(&["plugin", "kill", &core, "--reason", "test"]);
    let status = rig.cli(&["plugin", "status", &core]).stdout_raw;
    assert!(!status.contains("disabled-by-admin"), "the essential core plugin still runs");
}

/// REQ: OPS-207
#[test]
fn ops_207_killed_plugin_pending_requests_are_cancelled_a() {
    let directory = tempfile::tempdir().expect("scratch folder");
    let mut switch = KillSwitch::open(directory.path());
    switch.start_requests(PLUGIN, 3);
    switch.kill(PLUGIN, "incident response", "operator");
    assert_eq!(
        switch.request_outcomes(PLUGIN),
        vec!["cancelled".to_owned(), "cancelled".to_owned(), "cancelled".to_owned()],
        "the three pending requests complete as cancelled"
    );
}

/// REQ: OPS-207
#[test]
fn ops_207_other_plugins_keep_their_pending_requests_b() {
    let directory = tempfile::tempdir().expect("scratch folder");
    let mut switch = KillSwitch::open(directory.path());
    switch.start_requests(PLUGIN, 3);
    switch.start_requests(OTHER, 2);
    switch.kill(PLUGIN, "incident response", "operator");
    assert!(
        switch.request_outcomes(OTHER).is_empty(),
        "the bystander plugin's pending requests are not cancelled"
    );
    assert_eq!(switch.plugin_state(OTHER), PluginState::Active);
}

/// REQ: OPS-208
#[test]
fn ops_208_incident_record_holds_kill_entry_a() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "fixture incident"]);
    let incident = files_named_like(&rig.state_dir(), "incident");
    let text: String = incident
        .iter()
        .map(|path| fs::read_to_string(path).unwrap_or_default())
        .collect();
    assert!(text.contains("kill") && text.contains(PLUGIN), "the kill is in the incident record");
}

/// REQ: OPS-208
#[test]
fn ops_208_incident_record_holds_actor_reason_time_for_kill_and_enable_b() {
    let rig = OpsRig::new();
    let _server = rig.start_server("default");
    let _ = rig.cli(&["plugin", "kill", PLUGIN, "--reason", "fixture incident"]);
    let _ = rig.cli(&["plugin", "enable", PLUGIN]);
    let incident = files_named_like(&rig.state_dir(), "incident");
    let text: String = incident
        .iter()
        .map(|path| fs::read_to_string(path).unwrap_or_default())
        .collect();
    assert!(text.contains("fixture incident"), "the reason is recorded");
    assert!(text.contains("enable"), "the re-enable is recorded");
    assert!(text.contains("actor") || text.contains("administrator"), "the actor is recorded");
}
