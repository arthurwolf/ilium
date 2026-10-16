//! Port twins whose original tests need testkit items the skeleton lacks (see ../API_GAPS_gapfill.md).

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

const HOOK_COMMAND: &str = "ilium progress guard || true";

fn install(host: &mut FakeHost) -> bool {
    let invocation = host.cli().invoke(&["progress", "guard", "--install"], b"");
    invocation.exit_code() == Some(0)
}

fn settings_path(host: &FakeHost) -> std::path::PathBuf {
    host.project_root().join(".claude").join("settings.local.json")
}

fn read_settings(host: &FakeHost) -> Value {
    serde_json::from_str(&std::fs::read_to_string(settings_path(host)).expect("settings file")).expect("json")
}

fn guard_commands(root: &Value) -> Vec<String> {
    root["hooks"]["PreToolUse"].as_array().cloned().unwrap_or_default().iter()
        .flat_map(|group| group["hooks"].as_array().cloned().unwrap_or_default())
        .filter_map(|hook| hook["command"].as_str().map(str::to_string))
        .filter(|command| command.contains("ilium progress guard"))
        .collect()
}

fn write_settings(host: &FakeHost, contents: &str) {
    let path = settings_path(host);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::installer_writes_one_bash_guard_and_is_idempotent
#[test]
fn port_060_the_installer_writes_one_bash_guard_and_is_idempotent() {
    let mut host = activated_client();
    assert!(install(&mut host));
    assert!(install(&mut host));
    let root = read_settings(&host);
    assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
    assert_eq!(root["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(root["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 10);
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::installer_rewrites_the_legacy_command_and_keeps_other_hooks
#[test]
fn port_060_the_installer_rewrites_the_legacy_command_and_keeps_other_hooks() {
    let mut host = activated_client();
    write_settings(&host, r#"{"model":"x","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"ilium progress guard","timeout":10}]},{"matcher":"Edit","hooks":[{"type":"command","command":"other"}]}]}}"#);
    assert!(install(&mut host));
    let root = read_settings(&host);
    assert_eq!(root["model"], "x");
    assert_eq!(guard_commands(&root), vec![HOOK_COMMAND.to_string()]);
    assert_eq!(root["hooks"]["PreToolUse"][1]["hooks"][0]["command"], "other");
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::installer_keeps_a_user_written_guard_wrapper
#[test]
fn port_060_the_installer_keeps_a_user_written_guard_wrapper_byte_for_byte() {
    let mut host = activated_client();
    let wrapper = "ilium progress --help 2>/dev/null | grep -qE '^ +guard( |$)' || exit 0; exec ilium progress guard";
    let original = json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": wrapper, "timeout": 10}]}]}});
    write_settings(&host, &original.to_string());
    assert!(install(&mut host));
    assert_eq!(read_settings(&host), original);
}

/// REQ: PORT:ilium-client/src/progress_guard.rs::installer_rejects_a_settings_file_that_is_not_an_object
#[test]
fn port_060_the_installer_rejects_a_settings_file_that_is_not_an_object_and_leaves_it_alone() {
    let mut host = activated_client();
    write_settings(&host, "[1]");
    assert!(!install(&mut host), "a non-object settings file is refused");
    assert_eq!(std::fs::read_to_string(settings_path(&host)).unwrap(), "[1]");
}

/// REQ: PORT:ilium-client/src/app.rs::completed_progress_expiry_is_independent_in_both_split_orientations
#[test]
fn port_061_expiry_of_a_done_footer_is_independent_of_a_running_one_in_both_split_orientations() {
    for orientation in ["vertical", "horizontal"] {
        let mut host = activated_client();
        host.set_split_orientation(orientation);
        show(&mut host, vec![
            monitor_with(1, "split-done", "done", 100.0, "retained split result", "", Value::Null),
            monitor_with(2, "split-busy", "running", 100.0, "busy split result", "", Value::Null),
        ]);
        assert!(footer(&mut host, 80, 12).contains("busy split result"), "{orientation}");
        pass(&mut host, 61_000);
        let text = footer(&mut host, 80, 12);
        assert!(!text.contains("retained split result"), "{orientation}: the done result expired: {text}");
        assert!(text.contains("busy split result"), "{orientation}: the running one stays: {text}");
    }
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::failed_progress_snapshot_write_rejects_replacement_and_preserves_old_monitor
#[test]
fn port_059_a_failed_durable_write_rejects_the_replacement_and_keeps_the_old_monitor() {
    let (mut host, pane) = host_with_pane();
    let kept = register(&mut host, pane, PROBE, &running("preserved-job", 23, "running"), &[]);
    host.fail_next_durable_write();
    expect_probe(&mut host, "/opt/other.sh", &running("unacknowledged-job", 24, "running"));
    let run = progress(&mut host, pane, &["set", "--command", "/opt/other.sh"]);
    assert_eq!(run.exit, Some(2));
    assert!(!code(&run).is_empty(), "a rejection record names a code: {}", run.stdout);
    let record = status(&mut host, pane);
    assert_eq!(record["monitor_count"], 1);
    assert_eq!(record["monitors"][0]["monitor_id"].as_u64(), Some(kept));
    assert_eq!(record["monitors"][0]["report"]["job_id"], "preserved-job");
}

/// REQ: PORT:ilium-server/src/startup_progress.rs::native_publication_runs_off_caller_and_preserves_record_readback
#[test]
fn port_061_a_published_startup_record_reads_back_and_finish_removes_the_marker() {
    let mut host = server_host();
    let phase = host.begin_startup_phase();
    host.startup_publish(phase, "Restoring panes", "Exact pane", 2, 7);
    host.settle();
    assert_eq!(host.startup_marker(), Some(("Restoring panes".to_string(), "Exact pane".to_string(), 2, 7)));
    host.startup_finish(phase);
    host.settle();
    assert_eq!(host.startup_marker(), None);
}

/// REQ: PORT:ilium-server/src/startup_progress.rs::finish_fences_a_late_write_and_cannot_clear_a_newer_phase
#[test]
fn port_061_finish_fences_a_late_write_and_cannot_clear_a_newer_phase() {
    let mut host = server_host();
    let old = host.begin_startup_phase();
    host.block_next_startup_write();
    host.startup_publish(old, "Old", "Delayed", 0, 1);
    host.startup_finish(old);
    host.release_startup_write();
    host.settle();
    assert_eq!(host.startup_marker(), None, "a retired phase cannot resurrect the marker");
    let current = host.begin_startup_phase();
    host.startup_publish(current, "Current", "Retained", 1, 1);
    host.settle();
    host.startup_finish(old);
    host.settle();
    assert_eq!(host.startup_marker(), Some(("Current".to_string(), "Retained".to_string(), 1, 1)));
}

/// REQ: PORT:ilium-server/src/startup_progress.rs::finish_closes_its_phase_even_if_a_younger_update_returns_first
#[test]
fn port_061_finish_closes_its_phase_even_if_a_younger_update_returns_first() {
    let mut host = server_host();
    let phase = host.begin_startup_phase();
    host.block_next_startup_write();
    host.startup_publish(phase, "First", "Older", 0, 2);
    host.startup_publish(phase, "Second", "Younger", 1, 2);
    host.startup_finish(phase);
    host.release_startup_write();
    host.settle();
    assert_eq!(host.startup_marker(), None, "the phase is closed once finish was called");
}

/// REQ: PORT:ilium-server/src/startup_progress.rs::cancellation_keeps_a_blocked_callback_admitted_until_native_return
#[test]
fn port_061_cancellation_keeps_a_blocked_callback_admitted_until_the_native_write_returns() {
    let mut host = server_host();
    let phase = host.begin_startup_phase();
    host.block_next_startup_write();
    host.startup_publish(phase, "Blocked", "Write", 0, 1);
    host.cancel_startup();
    assert!(host.startup_callback_admitted(), "still admitted while the native write is blocked");
    host.release_startup_write();
    host.settle();
    assert!(!host.startup_callback_admitted(), "released once the native write returned");
}
