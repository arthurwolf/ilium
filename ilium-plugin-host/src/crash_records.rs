//! Crash records of plugin helper failures (core-plugin-diagnostics.md, "Crash records";
//! USR-227..229). Records are written to the server state folder, never to a plugin data folder.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

/// Keeps the records no older than 30 days and at most 200, dropping the oldest first (USR-228).
pub fn retain(_records: Vec<CrashRecord>, _now: SystemTime) -> Vec<CrashRecord> {
    unimplemented!("host::crash_records::retain")
}

/// The one file that holds the records, directly inside the server state folder (USR-229).
pub fn records_path(_state_dir: &Path) -> PathBuf {
    unimplemented!("host::crash_records::records_path")
}

/// One line of the records file, with the fields of USR-227 and no payload (USR-227).
pub fn format_record(_record: &CrashRecord) -> String {
    unimplemented!("host::crash_records::format_record")
}
