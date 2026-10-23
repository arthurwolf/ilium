//! Manual page `guide-admin-failure-runbook`: `ilium plugin doctor` and the stale-lock rule
//! (HST-696, 697).

use std::time::Duration;

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-696
#[test]
fn hst_696_doctor_reports_findings_changes_nothing_without_fix_and_fix_is_reversible_only() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_stale_lock_file(NOTES);
    rig.plant_orphan_staging_folder(NOTES);
    let before = rig.snapshot_plugin_tree();
    let report = rig.cli(&["plugin", "doctor"]);
    assert_eq!(report.exit_code, 1, "exit 1 when findings exist");
    assert_eq!(
        rig.snapshot_plugin_tree(),
        before,
        "nothing changed without --fix"
    );
    for finding in &report.stdout_jsonl {
        assert!(
            finding["code"].is_string(),
            "one line per finding with a stable code"
        );
    }
    let fixed = rig.cli(&["plugin", "doctor", "--fix"]);
    assert_eq!(fixed.exit_code, 2, "exit 2 when findings were fixed");
    assert!(
        !rig.stale_lock_exists(NOTES),
        "stale lock removed as a reversible action"
    );
    assert!(
        rig.doctor_fix_actions_are_reversible(),
        "only move aside, rebuild index and remove stale lock are performed"
    );
}

/// REQ: HST-697
#[test]
fn hst_697_lock_is_stale_only_when_holder_is_gone_rebooted_or_silent_and_waiters_fail_after_30_s() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let live = rig.hold_lock_with_holder(NOTES, "live", 5);
    let waited = rig.cli(&["plugin", "install", NOTES, "--yes"]);
    assert_eq!(waited.exit_code, 1, "waiting on a live holder fails");
    assert!(
        waited.stderr.contains("registry.locked"),
        "fails with registry.locked"
    );
    assert!(waited.stderr.contains("live"), "names the holder");
    assert!(
        rig.lock_waited_at_least(Duration::from_secs(30)),
        "fails only after 30 s"
    );
    live.release();
    rig.hold_lock_with_holder(NOTES, "silent", 61);
    rig.start_server("default");
    assert!(
        rig.log_lines_with_code("lock.takeover").len() == 1,
        "takeover of a stale lock logged once"
    );
    let takeover = rig.log_lines_with_code("lock.takeover");
    assert!(
        takeover[0]["pid"].is_number() && takeover[0]["boot_id"].is_string(),
        "log carries the old holder's pid and boot id"
    );
}
