//! Kill policy and on-disk format numbers (LIF-301..305).
//!
//! Assumption: a policy kill entry is written as `[plugins."<id>"] kill = "<version range>"`,
//! following the `[plugins."<id>"]` table convention in API_GAPS.md. The manual names the `kill`
//! key and the reason and log codes, not the TOML shape.

use ilium_plugin_api::Side;

use crate::common::*;

/// One policy file that kills `version_range` of `plugin_id`.
fn kill_policy(plugin_id: &str, version_range: &str) -> String {
    format!("[plugins.\"{plugin_id}\"]\nkill = \"{version_range}\"\n")
}

/// The count of `policy.kill` log records on a server.
fn policy_kill_count(server: &ServerHandle) -> usize {
    server
        .log_codes()
        .iter()
        .filter(|code| code.as_str() == "policy.kill")
        .count()
}

/// REQ: LIF-301
#[test]
fn lif_301_kill_entry_stops_running_instance_with_reason_policy() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.reload_policy();
    assert_eq!(server.inactive_reason(NOTES).as_deref(), Some("policy"));
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-301
#[test]
fn lif_301_kill_entry_logs_policy_kill_once_per_plugin_id() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.reload_policy();
    rig.reload_policy();
    assert_eq!(policy_kill_count(&server), 1);
}

/// REQ: LIF-301
#[test]
fn lif_301_killed_version_is_not_activated_on_a_fresh_start() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    let server = rig.start_server("default");
    assert_eq!(server.active_version(NOTES), None);
    assert_eq!(server.inactive_reason(NOTES).as_deref(), Some("policy"));
}

/// REQ: LIF-302
#[test]
fn lif_302_lifting_a_kill_starts_the_plugin_again_without_reinstall() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.reload_policy();
    rig.write_policy("");
    rig.reload_policy();
    server.apply_pending();
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: LIF-302
#[test]
fn lif_302_killed_version_keeps_its_storage_unchanged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    rig.write_storage(NOTES, "global", "draft", b"kept");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.reload_policy();
    rig.write_policy("");
    rig.reload_policy();
    server.apply_pending();
    assert_eq!(
        rig.read_storage(NOTES, "global", "draft"),
        Some(b"kept".to_vec())
    );
}

/// REQ: LIF-303
#[test]
fn lif_303_kill_takes_effect_within_60_seconds_without_a_server_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_polling("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.advance(secs(60));
    assert_eq!(server.inactive_reason(NOTES).as_deref(), Some("policy"));
    assert!(server.is_running());
}

/// REQ: LIF-303
#[test]
fn lif_303_kill_applies_before_the_crash_loop_guard_counts_a_failure() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_polling("default");
    rig.write_policy(&kill_policy(NOTES, ">=1.0.0, <2.0.0"));
    rig.advance(secs(60));
    assert_eq!(server.failure_count(NOTES, Side::Server), 0);
}

/// Writes `installed.json` with a `format` number that no host knows yet (LIF-304, LIF-305).
/// The host's own format number N is below this value, so the file is always newer.
fn plant_newer_installed_json(rig: &OpsRig) -> std::path::PathBuf {
    let path = rig.state_dir().join("installed.json");
    let text = std::fs::read(&path).expect("installed.json exists after an install");
    let mut document: serde_json::Value =
        serde_json::from_slice(&text).expect("installed.json is JSON");
    document["format"] = serde_json::json!(9999);
    std::fs::write(&path, serde_json::to_vec(&document).expect("serialize")).expect("write");
    path
}

/// REQ: LIF-304
#[test]
fn lif_304_host_does_not_rewrite_a_state_file_with_a_newer_format() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let path = plant_newer_installed_json(&rig);
    let before = std::fs::read(&path).expect("read");
    let _server = rig.start_server("default");
    let after = std::fs::read(&path).expect("read");
    assert_eq!(before, after);
}

/// REQ: LIF-304
#[test]
fn lif_304_host_does_not_parse_a_state_file_with_a_newer_format() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    plant_newer_installed_json(&rig);
    let server = rig.start_server("default");
    assert_eq!(server.active_version(NOTES), None);
}

/// REQ: LIF-305
#[test]
fn lif_305_newer_installed_json_disables_affected_plugins_and_logs_format_newer() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    plant_newer_installed_json(&rig);
    let server = rig.start_server("default");
    assert!(!server.is_enabled(NOTES));
    assert!(server
        .log_codes()
        .iter()
        .any(|code| code == "host.format-newer"));
}

/// REQ: LIF-305
#[test]
fn lif_305_newer_installed_json_is_not_deleted_or_truncated() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let path = plant_newer_installed_json(&rig);
    let before = std::fs::read(&path).expect("read");
    let _server = rig.start_server("default");
    let after = std::fs::read(&path).expect("file must still exist");
    assert_eq!(before, after);
}
