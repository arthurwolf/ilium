//! Sandbox helper crash forensics and core-dump policy (HST-518, HST-519). Skeleton: every
//! The crash record carries only counters and the call name, never payloads or paths.

use std::path::PathBuf;

/// Stable condition id of a helper death in the plugin log (HST-518).
const HELPER_DIED_CONDITION: &str = "helper.died";

/// Helper log lines kept in a crash record (HST-518).
const RETAINED_LOG_LINES: usize = 20;

/// Days an enabled core dump is kept (HST-519).
const CORE_DUMP_RETENTION_DAYS: u32 = 7;

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
pub fn record_helper_death(death: &HelperDeath) -> CrashRecord {
    let (exit_code, terminating_signal) = match death.exit {
        HelperExit::Code(code) => (Some(code), None),
        HelperExit::Signal(signal) => (None, Some(signal)),
    };
    let first_kept = death.log_lines.len().saturating_sub(RETAINED_LOG_LINES);
    let retained_log_lines: Vec<String> = death.log_lines[first_kept..].to_vec();
    // The call name is a manifest export name; anything after the first whitespace could be
    // payload text, so only the leading token is kept.
    let call_name = death
        .call_name
        .as_deref()
        .map(|name| name.split_whitespace().next().unwrap_or("").to_string());
    let exit_text = match death.exit {
        HelperExit::Code(code) => format!("exit_code={code}"),
        HelperExit::Signal(signal) => format!("signal={signal}"),
    };
    let call_text = match (&call_name, death.call_elapsed_ms) {
        (Some(name), Some(elapsed)) => format!(" call={name} call_elapsed_ms={elapsed}"),
        (Some(name), None) => format!(" call={name}"),
        _ => String::new(),
    };
    let rendered = format!(
        "{HELPER_DIED_CONDITION} plugin={} {exit_text}{call_text} uptime_s={} peak_memory_bytes={} log_lines={}",
        death.plugin_id,
        death.helper_uptime_seconds,
        death.peak_memory_bytes,
        retained_log_lines.len()
    );
    CrashRecord {
        condition_id: HELPER_DIED_CONDITION.to_string(),
        exit_code,
        terminating_signal,
        call_name,
        call_elapsed_ms: death.call_elapsed_ms,
        helper_uptime_seconds: death.helper_uptime_seconds,
        peak_memory_bytes: death.peak_memory_bytes,
        retained_log_lines,
        rendered,
    }
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
pub fn core_dump_setting(policy: &CoreDumpPolicy) -> CoreDumpSetting {
    if !policy.administrator_key_enabled {
        return CoreDumpSetting {
            enabled: false,
            directory: None,
            directory_mode: None,
            retention_days: None,
        };
    }
    CoreDumpSetting {
        enabled: true,
        directory: Some(policy.directory.clone()),
        directory_mode: Some(0o700),
        retention_days: Some(CORE_DUMP_RETENTION_DAYS),
    }
}

/// True once a core dump is older than its retention period (HST-519).
pub fn core_dump_expired(setting: &CoreDumpSetting, age_days: u32) -> bool {
    match setting.retention_days {
        Some(retention_days) => age_days > retention_days,
        None => false,
    }
}

/// A core dump may be attached to a crash report only when the user chose to attach it
/// (HST-519).
pub fn may_attach_core_dump(setting: &CoreDumpSetting, user_chose_to_attach: bool) -> bool {
    setting.enabled && user_chose_to_attach
}
