//! reference-host-helper.md rows HST-518 and HST-519: helper crash record and core-dump policy.

use std::path::PathBuf;

use ilium_plugin_host::helper_forensics::{
    core_dump_expired, core_dump_setting, may_attach_core_dump, record_helper_death, CoreDumpPolicy,
    HelperDeath, HelperExit,
};

const PAYLOAD_MARKER: &str = "PAYLOAD-MARKER-7f3a";

fn death_with(exit: HelperExit, log_lines: Vec<String>) -> HelperDeath {
    HelperDeath {
        plugin_id: "acme.notes".to_string(),
        exit,
        call_name: Some("on-save".to_string()),
        call_elapsed_ms: Some(1500),
        helper_uptime_seconds: 3600,
        peak_memory_bytes: 64 * 1024 * 1024,
        log_lines,
    }
}

fn numbered_lines(count: usize) -> Vec<String> {
    (1..=count).map(|number| format!("line {number}")).collect()
}

/// REQ: HST-518
#[test]
fn hst_518_an_exit_code_is_recorded_with_every_listed_field() {
    let record = record_helper_death(&death_with(HelperExit::Code(3), numbered_lines(2)));
    assert_eq!(record.exit_code, Some(3));
    assert_eq!(record.terminating_signal, None);
    assert_eq!(record.call_name.as_deref(), Some("on-save"));
    assert_eq!(record.call_elapsed_ms, Some(1500));
    assert_eq!(record.helper_uptime_seconds, 3600);
    assert_eq!(record.peak_memory_bytes, 64 * 1024 * 1024);
}

/// REQ: HST-518
#[test]
fn hst_518_a_segmentation_fault_is_recorded_as_its_terminating_signal() {
    let record = record_helper_death(&death_with(HelperExit::Signal(11), Vec::new()));
    assert_eq!(record.terminating_signal, Some(11));
    assert_eq!(record.exit_code, None);
}

/// REQ: HST-518
#[test]
fn hst_518_a_sigkill_is_recorded_as_its_terminating_signal() {
    let record = record_helper_death(&death_with(HelperExit::Signal(9), Vec::new()));
    assert_eq!(record.terminating_signal, Some(9));
}

/// REQ: HST-518
#[test]
fn hst_518_only_the_last_20_helper_log_lines_are_retained() {
    let record = record_helper_death(&death_with(HelperExit::Code(1), numbered_lines(25)));
    assert_eq!(record.retained_log_lines.len(), 20);
    assert_eq!(record.retained_log_lines.first().map(String::as_str), Some("line 6"));
}

/// REQ: HST-518
#[test]
fn hst_518_the_crash_record_contains_no_payload_text() {
    let mut death = death_with(HelperExit::Signal(9), vec!["ok".to_string()]);
    death.call_name = Some(format!("on-save {PAYLOAD_MARKER}"));
    let record = record_helper_death(&death);
    assert!(!record.rendered.contains(PAYLOAD_MARKER));
}

fn policy(enabled: bool) -> CoreDumpPolicy {
    CoreDumpPolicy {
        administrator_key_enabled: enabled,
        directory: PathBuf::from("/var/lib/ilium/cores"),
    }
}

/// REQ: HST-519
#[test]
fn hst_519_core_dumps_are_disabled_under_the_default_policy() {
    let setting = core_dump_setting(&policy(false));
    assert!(!setting.enabled);
    assert_eq!(setting.directory, None);
}

/// REQ: HST-519
#[test]
fn hst_519_an_enabled_core_dump_goes_to_an_owner_only_directory() {
    let setting = core_dump_setting(&policy(true));
    assert!(setting.enabled);
    assert_eq!(setting.directory_mode, Some(0o700));
}

/// REQ: HST-519
#[test]
fn hst_519_an_enabled_core_dump_is_deleted_after_seven_days() {
    let setting = core_dump_setting(&policy(true));
    assert_eq!(setting.retention_days, Some(7));
    assert!(!core_dump_expired(&setting, 6));
    assert!(core_dump_expired(&setting, 8));
}

/// REQ: HST-519
#[test]
fn hst_519_a_core_dump_is_not_attached_to_a_crash_report_without_user_choice() {
    let setting = core_dump_setting(&policy(true));
    assert!(!may_attach_core_dump(&setting, false));
    assert!(may_attach_core_dump(&setting, true));
}
