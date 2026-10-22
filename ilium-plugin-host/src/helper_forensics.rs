//! Sandbox helper crash forensics and core-dump policy (HST-518, HST-519). Skeleton: every
//! function with behaviour is `unimplemented!` until the implementation phase.

use std::path::PathBuf;

/// How a helper process ended (HST-518).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperExit {
    Code(i32),
    Signal(i32),
}

/// The facts the host knows when a helper dies (HST-518).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperDeath {
    pub plugin_id: String,
    pub exit: HelperExit,
    pub call_name: Option<String>,
    pub call_elapsed_ms: Option<u64>,
    pub helper_uptime_seconds: u64,
    pub peak_memory_bytes: u64,
    pub log_lines: Vec<String>,
}

/// The record written to the plugin log under the stable condition id (HST-518).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashRecord {
    pub condition_id: String,
    pub exit_code: Option<i32>,
    pub terminating_signal: Option<i32>,
    pub call_name: Option<String>,
    pub call_elapsed_ms: Option<u64>,
    pub helper_uptime_seconds: u64,
    pub peak_memory_bytes: u64,
    pub retained_log_lines: Vec<String>,
    pub rendered: String,
}

/// Builds the crash record. It keeps the last 20 helper log lines and no payloads, paths or
/// terminal text (HST-518).
pub fn record_helper_death(_death: &HelperDeath) -> CrashRecord {
    unimplemented!("host::helper_forensics::record_helper_death")
}

/// The administrator's core-dump policy (HST-519).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDumpPolicy {
    pub administrator_key_enabled: bool,
    pub directory: PathBuf,
}

/// The effective core-dump setting for helpers (HST-519).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDumpSetting {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
    pub directory_mode: Option<u32>,
    pub retention_days: Option<u32>,
}

/// Core dumps are off unless the administrator policy key enables them; an enabled dump goes to
/// an owner-only directory and is kept 7 days (HST-519).
pub fn core_dump_setting(_policy: &CoreDumpPolicy) -> CoreDumpSetting {
    unimplemented!("host::helper_forensics::core_dump_setting")
}

/// True once a core dump is older than its retention period (HST-519).
pub fn core_dump_expired(_setting: &CoreDumpSetting, _age_days: u32) -> bool {
    unimplemented!("host::helper_forensics::core_dump_expired")
}

/// A core dump may be attached to a crash report only when the user chose to attach it
/// (HST-519).
pub fn may_attach_core_dump(_setting: &CoreDumpSetting, _user_chose_to_attach: bool) -> bool {
    unimplemented!("host::helper_forensics::may_attach_core_dump")
}
