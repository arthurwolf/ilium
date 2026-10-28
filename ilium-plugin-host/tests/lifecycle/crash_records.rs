//! Crash records of helper failures: USR-227..229 of reference-requirements-user-admin.md
//! (core-plugin-diagnostics.md, "Crash records"). Each test drives the real record
//! functions and the boot harness.

use std::path::Path;
use std::time::{Duration, SystemTime};

use ilium_plugin_api::{ProcessKind, Side, Tier};
use ilium_plugin_host::crash_records::{
    format_record, records_path, retain, CrashReason, CrashRecord,
};
use ilium_plugin_host::testing::boot::{BootHarness, PluginSpec};

const ID: &str = "example.notes";
const VERSION: &str = "0.1.0";
const DAY_SECS: u64 = 86_400;

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

fn record_at(time_utc: SystemTime) -> CrashRecord {
    CrashRecord {
        time_utc,
        plugin_id: ID.to_owned(),
        plugin_version: VERSION.to_owned(),
        side: Side::Server,
        reason: CrashReason::Crashed,
        exit_status: Some(1),
        export_name: Some("on-events".to_owned()),
    }
}

/// REQ: USR-227
#[test]
fn usr_227_helper_kill_appends_one_record_with_the_required_fields() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(PluginSpec::new(ID, Tier::T2).sides(&[Side::Server]))
        .build();
    run.start();
    run.kill_helper();
    let records = run.crash_records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.plugin_id, ID);
    assert_eq!(record.plugin_version, VERSION);
    assert_eq!(record.side, Side::Server);
    assert_eq!(record.reason, CrashReason::Killed);
    assert!(record.exit_status.is_some());
    assert!(record.export_name.is_some());
}

/// REQ: USR-227
#[test]
fn usr_227_a_record_line_has_a_utc_time_and_no_payload_field() {
    let line = format_record(&record_at(now()));
    assert!(line.contains(ID));
    assert!(line.contains(VERSION));
    assert!(
        line.ends_with('Z') || line.contains("Z "),
        "UTC time: {line}"
    );
    assert!(!line.to_lowercase().contains("payload"));
}

/// REQ: USR-228
#[test]
fn usr_228_records_older_than_30_days_are_removed() {
    let old = record_at(now() - Duration::from_secs(31 * DAY_SECS));
    let fresh = record_at(now() - Duration::from_secs(DAY_SECS));
    let kept = retain(vec![old, fresh.clone()], now());
    assert_eq!(kept, vec![fresh]);
}

/// REQ: USR-228
#[test]
fn usr_228_at_most_200_records_are_kept_and_the_oldest_go_first() {
    let records: Vec<CrashRecord> = (0..201)
        .map(|index| record_at(now() - Duration::from_secs(index)))
        .collect();
    let kept = retain(records, now());
    assert_eq!(kept.len(), 200);
    assert!(kept
        .iter()
        .all(|record| record.time_utc > now() - Duration::from_secs(200)));
}

/// REQ: USR-229
#[test]
fn usr_229_records_live_directly_in_the_server_state_folder() {
    let state = Path::new("/home/example/.local/state/ilium");
    assert_eq!(records_path(state).parent(), Some(state));
}
