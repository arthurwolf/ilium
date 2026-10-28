//! Crash records of plugin helper failures (core-plugin-diagnostics.md, "Crash records";
//! USR-227..229). Records are written to the server state folder, never to a plugin data folder.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::Side;

/// Why a helper stopped (USR-227).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashReason {
    /// The helper exited with an error or a trap.
    Crashed,
    /// The helper was killed on purpose or by the watchdog.
    Killed,
    /// The helper was killed because it ran out of memory.
    OutOfMemory,
}

/// One helper failure. It holds no payloads (USR-227).
#[derive(Debug, Clone, PartialEq)]
pub struct CrashRecord {
    /// The moment of the failure, in UTC.
    pub time_utc: SystemTime,
    /// The plugin whose helper failed.
    pub plugin_id: String,
    /// The version of that plugin.
    pub plugin_version: String,
    /// The side whose helper failed.
    pub side: Side,
    /// Why the helper stopped.
    pub reason: CrashReason,
    /// The exit status, when the helper left one.
    pub exit_status: Option<i32>,
    /// The export that was running, when known.
    pub export_name: Option<String>,
}

/// Records older than this are dropped (USR-228).
const RETENTION: Duration = Duration::from_secs(30 * 86_400);
/// At most this many records are kept (USR-228).
const MAX_RECORDS: usize = 200;
/// The file inside the server state folder that holds the records (USR-229).
const RECORDS_FILE_NAME: &str = "helper-crashes.log";

/// Keeps the records no older than 30 days and at most 200, dropping the oldest first (USR-228).
/// The result is ordered oldest first.
pub fn retain(records: Vec<CrashRecord>, now: SystemTime) -> Vec<CrashRecord> {
    let oldest_allowed = now.checked_sub(RETENTION).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut kept: Vec<CrashRecord> = records
        .into_iter()
        .filter(|record| record.time_utc >= oldest_allowed)
        .collect();
    kept.sort_by_key(|record| record.time_utc);
    let excess = kept.len().saturating_sub(MAX_RECORDS);
    kept.drain(..excess);
    kept
}

/// The one file that holds the records, directly inside the server state folder (USR-229).
pub fn records_path(state_dir: &Path) -> PathBuf {
    state_dir.join(RECORDS_FILE_NAME)
}

/// One line of the records file, with the fields of USR-227 and no payload (USR-227).
pub fn format_record(record: &CrashRecord) -> String {
    let reason = match record.reason {
        CrashReason::Crashed => "crash",
        CrashReason::Killed => "kill",
        CrashReason::OutOfMemory => "out-of-memory",
    };
    let exit_status = record
        .exit_status
        .map_or_else(|| "none".to_string(), |status| status.to_string());
    let export_name = record.export_name.as_deref().unwrap_or("unknown");
    format!(
        "plugin={} version={} side={} reason={} exit={} export={} time={}",
        record.plugin_id,
        record.plugin_version,
        side_code(record.side),
        reason,
        exit_status,
        export_name,
        rfc3339_utc(record.time_utc),
    )
}

/// The manual's side code: `t1` server, `t2` client, `t3` scene (USR-227).
fn side_code(side: Side) -> &'static str {
    match side {
        Side::Server => "t1",
        Side::Client => "t2",
        Side::Scene => "t3",
    }
}

/// Formats a time as RFC 3339 in UTC with whole seconds, ending in `Z`.
fn rfc3339_utc(time: SystemTime) -> String {
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = seconds / 86_400;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        second_of_day % 3_600 / 60,
        second_of_day % 60
    )
}

/// Converts days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
