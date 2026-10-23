//! Counters, traces and the log ring buffer behind `ilium plugin trace/profile/logs`
//! (reference-cli.md).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::{Side, Tier};

use crate::calls::CallClass;

/// One captured log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// The plugin.
    pub plugin: String,
    /// Level name.
    pub level: String,
    /// The message.
    pub message: String,
}

impl LogLine {
    /// Whether the message contains `needle` (reference-cli.md, `ilium plugin logs` filtering).
    pub fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}

/// The ring buffer of recent log lines.
#[derive(Debug)]
pub struct LogRing;

impl LogRing {
    /// A ring that keeps at most `capacity` lines.
    pub fn new(_capacity: usize) -> LogRing {
        LogRing
    }

    /// Adds a line, dropping the oldest at capacity.
    pub fn push(&mut self, _line: LogLine) {
        unimplemented!("host::telemetry::LogRing::push")
    }

    /// The lines of a plugin, oldest first.
    pub fn lines_of(&self, _plugin: &str) -> Vec<LogLine> {
        unimplemented!("host::telemetry::LogRing::lines_of")
    }
}

/// Cuts one WASI stderr line to the log line limit (reference-host-runtime.md HST-078).
pub fn cut_wasi_line(_line: &str) -> String {
    unimplemented!("host::telemetry::cut_wasi_line")
}

// ---------------------------------------------------------------------------------------------
// Near-miss tracking (reference-host-budgets.md HST-313).
// ---------------------------------------------------------------------------------------------

/// Counts calls that used 80 percent or more of their deadline within a five-minute window
/// (reference-host-budgets.md HST-313).
#[derive(Debug, Default)]
pub struct NearMissTracker;

impl NearMissTracker {
    /// An empty tracker.
    pub fn new() -> NearMissTracker {
        NearMissTracker
    }

    /// Records one call; returns whether it was a near miss (HST-313).
    pub fn record_call(&mut self, _now_ms: u64, _deadline_ms: u64, _took_ms: u64) -> bool {
        unimplemented!("host::telemetry::NearMissTracker::record_call")
    }

    /// Near misses inside the window ending at `now_ms` (HST-313).
    pub fn count_in_window(&self, _now_ms: u64) -> usize {
        unimplemented!("host::telemetry::NearMissTracker::count_in_window")
    }

    /// Whether the plugin counts as degraded at `now_ms` (HST-313).
    pub fn is_degraded(&self, _now_ms: u64) -> bool {
        unimplemented!("host::telemetry::NearMissTracker::is_degraded")
    }
}

// ---------------------------------------------------------------------------------------------
// Per-side counters (reference-host-telemetry.md HST-350..363).
// ---------------------------------------------------------------------------------------------

/// The counter names of one hour bucket, in record order (reference-host-telemetry.md HST-353).
pub const COUNTER_NAMES: [&str; 16] = [
    "calls",
    "duration",
    "near_misses",
    "breaches",
    "traps",
    "queue_depth",
    "events_dropped",
    "events_coalesced",
    "quota_rejections",
    "rate_limited",
    "cpu_ms",
    "memory_peak",
    "helper_round_trips",
    "render_ms",
    "restarts",
    "failures",
];

/// Upper bounds of the duration histogram buckets in milliseconds; a 13th bucket is open-ended
/// (reference-host-telemetry.md HST-352).
pub const DURATION_BUCKET_BOUNDS_MS: [f64; 12] = [
    0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 500.0, 1000.0,
];

/// The memory one side's telemetry may hold (reference-host-telemetry.md HST-351).
pub const TELEMETRY_MAX_BYTES_PER_SIDE: usize = 64 * 1024;

/// Whether a deadline breach was hard (call killed) or soft (reference-host-telemetry.md HST-352).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BreachKind {
    /// The hard deadline.
    Hard,
    /// The soft deadline.
    Soft,
}

/// The counters of one hour of one side (reference-host-telemetry.md HST-352, HST-354).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HourBucket {
    /// Start of the hour, milliseconds since the epoch.
    pub hour_start_ms: u64,
    counters: std::collections::BTreeMap<String, u64>,
}

impl HourBucket {
    /// Calls of one class.
    pub fn calls(&self, _class: CallClass) -> u64 {
        unimplemented!("host::telemetry::HourBucket::calls")
    }

    /// The 13-bucket duration histogram of one class.
    pub fn duration_histogram(&self, _class: CallClass) -> [u64; 13] {
        unimplemented!("host::telemetry::HourBucket::duration_histogram")
    }

    /// Near misses of one class.
    pub fn near_misses(&self, _class: CallClass) -> u64 {
        unimplemented!("host::telemetry::HourBucket::near_misses")
    }

    /// Deadline breaches of one class and kind.
    pub fn breaches(&self, _class: CallClass, _kind: BreachKind) -> u64 {
        unimplemented!("host::telemetry::HourBucket::breaches")
    }

    /// Traps by code.
    pub fn traps(&self, _code: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::traps")
    }

    /// The queue-depth high-water mark.
    pub fn queue_depth_high_water(&self) -> u64 {
        unimplemented!("host::telemetry::HourBucket::queue_depth_high_water")
    }

    /// Events dropped by topic.
    pub fn events_dropped(&self, _topic: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::events_dropped")
    }

    /// Events coalesced by topic.
    pub fn events_coalesced(&self, _topic: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::events_coalesced")
    }

    /// Quota rejections.
    pub fn quota_rejections(&self) -> u64 {
        unimplemented!("host::telemetry::HourBucket::quota_rejections")
    }

    /// Rate-limited calls by import name.
    pub fn rate_limited(&self, _import: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::rate_limited")
    }

    /// CPU milliseconds.
    pub fn cpu_ms(&self) -> u64 {
        unimplemented!("host::telemetry::HourBucket::cpu_ms")
    }

    /// Peak memory in bytes.
    pub fn memory_peak(&self) -> u64 {
        unimplemented!("host::telemetry::HourBucket::memory_peak")
    }

    /// Restarts by reason.
    pub fn restarts(&self, _reason: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::restarts")
    }

    /// Failures by reason.
    pub fn failures(&self, _reason: &str) -> u64 {
        unimplemented!("host::telemetry::HourBucket::failures")
    }

    /// The helper round-trip histogram.
    pub fn helper_round_trips(&self) -> [u64; 13] {
        unimplemented!("host::telemetry::HourBucket::helper_round_trips")
    }

    /// The render-time histogram of one view.
    pub fn render_ms(&self, _view: &str) -> [u64; 13] {
        unimplemented!("host::telemetry::HourBucket::render_ms")
    }
}

/// The in-memory counters of one plugin side; thread-safe, opens no socket
/// (reference-host-telemetry.md HST-350..352).
#[derive(Debug, Default)]
pub struct SideTelemetry;

impl SideTelemetry {
    /// Empty counters.
    pub fn new() -> SideTelemetry {
        SideTelemetry
    }

    /// Records one call: its duration, its deadline and the time (HST-352).
    pub fn record_call(
        &self,
        _class: CallClass,
        _took: Duration,
        _deadline: Duration,
        _now_ms: u64,
    ) {
        unimplemented!("host::telemetry::SideTelemetry::record_call")
    }

    /// Records a deadline breach (HST-352).
    pub fn record_breach(&self, _class: CallClass, _kind: BreachKind, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_breach")
    }

    /// Records a trap by code (HST-352).
    pub fn record_trap(&self, _code: &str, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_trap")
    }

    /// Records a queue-depth observation (HST-352).
    pub fn record_queue_depth(&self, _depth: u64, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_queue_depth")
    }

    /// Records dropped events of a topic (HST-352).
    pub fn record_event_dropped(&self, _topic: &str, _count: u64, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_event_dropped")
    }

    /// Records coalesced events of a topic (HST-352).
    pub fn record_event_coalesced(&self, _topic: &str, _count: u64, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_event_coalesced")
    }

    /// Records a quota rejection (HST-352).
    pub fn record_quota_rejection(&self, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_quota_rejection")
    }

    /// Records a rate-limited import call (HST-352).
    pub fn record_rate_limited(&self, _import: &str, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_rate_limited")
    }

    /// Adds CPU milliseconds (HST-352).
    pub fn record_cpu_ms(&self, _cpu_ms: u64, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_cpu_ms")
    }

    /// Records a memory observation; keeps the peak (HST-352).
    pub fn record_memory(&self, _bytes: u64, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_memory")
    }

    /// Records a helper round trip (HST-352).
    pub fn record_helper_round_trip(&self, _took: Duration, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_helper_round_trip")
    }

    /// Records a render of one view (HST-352).
    pub fn record_render(&self, _view: &str, _took: Duration, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_render")
    }

    /// Records a restart by reason (HST-352).
    pub fn record_restart(&self, _reason: &str, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_restart")
    }

    /// Records a failure by reason (HST-352).
    pub fn record_failure(&self, _reason: &str, _now_ms: u64) {
        unimplemented!("host::telemetry::SideTelemetry::record_failure")
    }

    /// The retained hour buckets at `now_ms`, oldest first; 24 hours are kept (HST-354).
    pub fn snapshot(&self, _now_ms: u64) -> Vec<HourBucket> {
        unimplemented!("host::telemetry::SideTelemetry::snapshot")
    }

    /// Bytes this side's telemetry holds (HST-351).
    pub fn memory_bytes(&self) -> usize {
        unimplemented!("host::telemetry::SideTelemetry::memory_bytes")
    }

    /// Ends the instance: one summary log line, or none without calls (HST-357).
    pub fn end_instance(&self, _now_ms: u64) -> Option<String> {
        unimplemented!("host::telemetry::SideTelemetry::end_instance")
    }
}

/// Who asks to read counters (reference-host-telemetry.md HST-356).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reader {
    /// The kernel.
    Kernel,
    /// The command line.
    Cli,
    /// The settings screens.
    Ui,
    /// A plugin, by id.
    Plugin(String),
}

/// Why a telemetry read was refused (reference-host-telemetry.md HST-356, HST-362).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryError {
    /// The stable error code.
    pub code: String,
    /// Human text.
    pub message: String,
}

/// The `plugin-info` summary a plugin may read (reference-host-telemetry.md HST-362).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    /// Calls in the retained window.
    pub calls: u64,
    /// Near misses in the retained window.
    pub near_misses: u64,
}

/// All sides' telemetry of one host (reference-host-telemetry.md HST-356).
#[derive(Debug, Default)]
pub struct TelemetryRegistry;

impl TelemetryRegistry {
    /// An empty registry.
    pub fn new() -> TelemetryRegistry {
        TelemetryRegistry
    }

    /// The telemetry of one side, created on first use.
    pub fn side(&self, _plugin: &str, _side: Side) -> std::sync::Arc<SideTelemetry> {
        unimplemented!("host::telemetry::TelemetryRegistry::side")
    }

    /// Reads the raw counters; only the kernel, the CLI and the UI may (HST-356).
    pub fn read(
        &self,
        _reader: &Reader,
        _plugin: &str,
        _side: Side,
        _now_ms: u64,
    ) -> Result<Vec<HourBucket>, TelemetryError> {
        unimplemented!("host::telemetry::TelemetryRegistry::read")
    }

    /// The summary `reader_plugin` may see of `target`; another plugin needs `plugins.manage` (HST-362).
    pub fn plugin_info(
        &self,
        _reader_plugin: &str,
        _target: &str,
        _has_manage: bool,
        _now_ms: u64,
    ) -> Result<Summary, TelemetryError> {
        unimplemented!("host::telemetry::TelemetryRegistry::plugin_info")
    }
}

/// One host log line about a plugin side (reference-host-telemetry.md HST-358).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLogLine {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The stable condition id, such as `trap.deadline`.
    pub id: String,
    /// The level word: `info`, `warn` or `error`.
    pub level: String,
    /// The detail text.
    pub detail: String,
}

/// Builds the host log line of a known condition id; unknown ids are refused (HST-358).
pub fn host_line(
    _plugin: &str,
    _side: Side,
    _id: &str,
    _detail: &str,
) -> Result<HostLogLine, TelemetryError> {
    unimplemented!("host::telemetry::host_line")
}

/// Admits at most ten lines per condition id, side and minute, then summarises (HST-358).
#[derive(Debug, Default)]
pub struct HostLogLimiter;

impl HostLogLimiter {
    /// An empty limiter.
    pub fn new() -> HostLogLimiter {
        HostLogLimiter
    }

    /// Whether a line of `id` on `side` may be written at `now_ms`.
    pub fn admit(&mut self, _side: Side, _id: &str, _now_ms: u64) -> bool {
        unimplemented!("host::telemetry::HostLogLimiter::admit")
    }

    /// The summary of lines suppressed in the last minute, once, or `None`.
    pub fn take_suppressed_summary(
        &mut self,
        _side: Side,
        _id: &str,
        _now_ms: u64,
    ) -> Option<String> {
        unimplemented!("host::telemetry::HostLogLimiter::take_suppressed_summary")
    }
}

/// One `host.slow-calls` report (reference-host-telemetry.md HST-359).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowCallReport {
    /// Always `host.slow-calls`.
    pub id: String,
    /// The three slowest calls of the minute, slowest first.
    pub top: Vec<(CallClass, Duration)>,
}

/// Collects calls that took over half their deadline and reports once a minute (HST-359).
#[derive(Debug, Default)]
pub struct SlowCallReporter;

impl SlowCallReporter {
    /// An empty reporter.
    pub fn new() -> SlowCallReporter {
        SlowCallReporter
    }

    /// Records one call with its duration and deadline.
    pub fn record(
        &mut self,
        _side: Side,
        _class: CallClass,
        _took: Duration,
        _deadline: Duration,
        _now_ms: u64,
    ) {
        unimplemented!("host::telemetry::SlowCallReporter::record")
    }

    /// The report due at `now_ms` for `side`, at most one per minute.
    pub fn due_report(&mut self, _side: Side, _now_ms: u64) -> Option<SlowCallReport> {
        unimplemented!("host::telemetry::SlowCallReporter::due_report")
    }
}

/// One recorded call span (reference-host-telemetry.md HST-360).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The call class.
    #[serde(serialize_with = "serialize_class")]
    pub class: CallClass,
    /// Time waiting in the queue.
    #[serde(rename = "queue_time_us", serialize_with = "serialize_micros")]
    pub queue_time: Duration,
    /// Time running.
    #[serde(rename = "run_time_us", serialize_with = "serialize_micros")]
    pub run_time: Duration,
    /// Imports forwarded to the helper during the call.
    pub forwarded_imports: u32,
}

fn serialize_micros<S: serde::Serializer>(
    duration: &Duration,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(duration.as_micros() as u64)
}

fn serialize_class<S: serde::Serializer>(
    class: &CallClass,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{class:?}").to_lowercase())
}

impl Span {
    /// The serialized field names, in order (HST-360).
    pub const FIELD_NAMES: [&'static str; 6] = [
        "plugin",
        "side",
        "class",
        "queue_time_us",
        "run_time_us",
        "forwarded_imports",
    ];
}

/// Records spans only when `ILIUM_PLUGIN_SPANS=1` (reference-host-telemetry.md HST-360).
#[derive(Debug, Default)]
pub struct SpanRecorder;

impl SpanRecorder {
    /// A recorder configured from the value of `ILIUM_PLUGIN_SPANS`.
    pub fn from_env(_value: Option<&str>) -> SpanRecorder {
        unimplemented!("host::telemetry::SpanRecorder::from_env")
    }

    /// Whether spans are recorded.
    pub fn is_enabled(&self) -> bool {
        unimplemented!("host::telemetry::SpanRecorder::is_enabled")
    }

    /// Records one span when enabled.
    pub fn record(&mut self, _span: Span) {
        unimplemented!("host::telemetry::SpanRecorder::record")
    }

    /// The recorded spans, in order.
    pub fn spans(&self) -> &[Span] {
        unimplemented!("host::telemetry::SpanRecorder::spans")
    }
}

/// The counters the health `Why` lines are computed from (reference-host-telemetry.md HST-363).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HealthInputs {
    /// Calls in the window.
    pub calls: u64,
    /// Near misses in the window.
    pub near_misses: u64,
    /// Quota rejections in the last minute.
    pub quota_rejections_last_minute: u64,
    /// Peak memory in bytes.
    pub memory_peak_bytes: u64,
    /// The memory limit in bytes.
    pub memory_limit_bytes: u64,
}

/// The `Why ...` lines of the health signals that fire (HST-363).
pub fn why_lines(_inputs: &HealthInputs) -> Vec<String> {
    unimplemented!("host::telemetry::why_lines")
}

// ---------------------------------------------------------------------------------------------
// Plugin log files (reference-debugging-tools.md DEV-650..662).
// ---------------------------------------------------------------------------------------------

/// A refusal with a stable code (reference-debugging-tools.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The stable code, such as `dev.id-installed`.
    pub code: String,
    /// Human text.
    pub message: String,
}

/// The folder holding the plugin logs of one session of one project (DEV-650).
pub fn plugin_log_dir(_state_dir: &Path, _project: &Path, _session: &str) -> PathBuf {
    unimplemented!("host::telemetry::plugin_log_dir")
}

/// A log level (reference-debugging-tools.md DEV-652).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    /// `trace`.
    Trace,
    /// `debug`.
    Debug,
    /// `info`.
    Info,
    /// `warn`.
    Warn,
    /// `error`.
    Error,
}

impl LogLevel {
    /// Every level, lowest first.
    pub const ALL: [LogLevel; 5] = [
        LogLevel::Trace,
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Error,
    ];

    /// The lowercase word of the level.
    pub fn as_str(self) -> &'static str {
        unimplemented!("host::telemetry::LogLevel::as_str")
    }
}

/// Which side wrote a log line; `Host` is the host itself (DEV-650).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogSide {
    /// The server component.
    Server,
    /// The client component.
    Client,
    /// The scene component.
    Scene,
    /// The host.
    Host,
}

impl LogSide {
    /// The column word of the side.
    pub fn as_str(self) -> &'static str {
        unimplemented!("host::telemetry::LogSide::as_str")
    }
}

/// One formatted log line with the fixed columns (DEV-650).
pub fn format_log_line(
    _at: &str,
    _level: LogLevel,
    _side: LogSide,
    _plugin: &str,
    _message: &str,
) -> String {
    unimplemented!("host::telemetry::format_log_line")
}

/// The longest message kept in a log line, in bytes (DEV-651).
pub const LOG_MESSAGE_MAX_BYTES: usize = 16_384;

/// The size at which a log file rotates (DEV-657).
pub const LOG_ROTATE_BYTES: u64 = 4 * 1024 * 1024;

/// Cuts a message to `LOG_MESSAGE_MAX_BYTES` on a character boundary (DEV-651).
pub fn truncate_message(_message: &str) -> String {
    unimplemented!("host::telemetry::truncate_message")
}

/// How a plugin log is written (DEV-652..655).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    /// Lines below this level are dropped.
    pub threshold: LogLevel,
    /// Plugin lines admitted per second before rate limiting.
    pub rate_per_second: u32,
}

impl Default for LogConfig {
    fn default() -> LogConfig {
        LogConfig {
            threshold: LogLevel::Info,
            rate_per_second: 100,
        }
    }
}

impl LogConfig {
    /// The configuration of a dev install: `debug` threshold (DEV-653).
    pub fn for_dev_install() -> LogConfig {
        unimplemented!("host::telemetry::LogConfig::for_dev_install")
    }

    /// The configuration for a plugin of a tier (DEV-653).
    pub fn for_tier(_tier: Tier) -> LogConfig {
        unimplemented!("host::telemetry::LogConfig::for_tier")
    }
}

/// An open plugin log file (DEV-650..658).
#[derive(Debug)]
pub struct PluginLog;

impl PluginLog {
    /// Opens (creating) the log of `plugin` under `dir`.
    pub fn open(_dir: &Path, _plugin: &str, _config: LogConfig) -> Result<PluginLog, Refusal> {
        unimplemented!("host::telemetry::PluginLog::open")
    }

    /// Writes a host line; returns whether it was written (DEV-652).
    pub fn write_host(
        &mut self,
        _at: &str,
        _now_ms: u64,
        _level: LogLevel,
        _message: &str,
    ) -> bool {
        unimplemented!("host::telemetry::PluginLog::write_host")
    }

    /// Writes a plugin line; returns whether it was written (DEV-652, DEV-655).
    pub fn write_plugin(
        &mut self,
        _at: &str,
        _now_ms: u64,
        _side: Side,
        _level: LogLevel,
        _message: &str,
    ) -> bool {
        unimplemented!("host::telemetry::PluginLog::write_plugin")
    }

    /// Flushes rate-limit summaries due at `now_ms` (DEV-655).
    pub fn tick(&mut self, _at: &str, _now_ms: u64) {
        unimplemented!("host::telemetry::PluginLog::tick")
    }

    /// Records that the log queue overflowed and `dropped` lines were lost (DEV-656).
    pub fn queue_full(&mut self, _at: &str, _now_ms: u64, _dropped: u64) {
        unimplemented!("host::telemetry::PluginLog::queue_full")
    }
}

/// The effective threshold from a flag, the settings value and the dev flag (DEV-653).
pub fn resolve_threshold(
    _flag: Option<LogLevel>,
    _setting: Option<&str>,
    _dev_install: bool,
) -> LogLevel {
    unimplemented!("host::telemetry::resolve_threshold")
}

/// The level and text of the restart line (DEV-659).
pub fn restart_log(_delay_s: u64, _failure: u32, _of: u32) -> (LogLevel, String) {
    unimplemented!("host::telemetry::restart_log")
}

/// The level and text of the disabled-after-failures line (DEV-659).
pub fn disabled_log(_failures: u32) -> (LogLevel, String) {
    unimplemented!("host::telemetry::disabled_log")
}

/// Marks the logs of an uninstalled plugin for expiry (DEV-661).
pub fn mark_uninstalled(_log_dir: &Path, _plugin: &str, _now_ms: u64) {
    unimplemented!("host::telemetry::mark_uninstalled")
}

/// Clears the expiry mark when the plugin is installed again (DEV-661).
pub fn mark_reinstalled(_log_dir: &Path, _plugin: &str) {
    unimplemented!("host::telemetry::mark_reinstalled")
}

/// Deletes the logs of plugins uninstalled long enough ago (DEV-661).
pub fn purge_expired_logs(_log_dir: &Path, _now_ms: u64) {
    unimplemented!("host::telemetry::purge_expired_logs")
}

/// A time as the local-time column of a log line (DEV-650).
pub fn format_local_time(_time: SystemTime) -> String {
    unimplemented!("host::telemetry::format_local_time")
}

// ---------------------------------------------------------------------------------------------
// Trace ring, inspector, memory view and profiler (reference-debugging-tools.md DEV-670..690).
// ---------------------------------------------------------------------------------------------

/// The records the trace ring keeps (DEV-670).
pub const TRACE_RING_CAPACITY: usize = 1024;

/// The kind of one trace record (DEV-670).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceKind {
    /// An event delivered to the plugin.
    Event,
    /// A submission.
    Submission,
    /// An outcome.
    Outcome,
    /// A host call.
    HostCall,
    /// A render.
    Render,
    /// A trap.
    Trap,
    /// A lifecycle step.
    Lifecycle,
}

impl TraceKind {
    /// The kebab-case word of the kind.
    pub fn as_str(self) -> &'static str {
        unimplemented!("host::telemetry::TraceKind::as_str")
    }
}

/// One trace record (DEV-670).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TraceRecord {
    /// Sequence number, increasing.
    pub seq: u64,
    /// Time in milliseconds.
    pub time_ms: u64,
    /// The kind.
    pub kind: TraceKind,
    /// The instance the record belongs to.
    pub instance: u64,
    /// The redacted detail.
    pub detail: serde_json::Value,
}

/// A ring of the newest `TRACE_RING_CAPACITY` records of the current instance (DEV-670, DEV-671).
#[derive(Debug, Default)]
pub struct TraceRing;

impl TraceRing {
    /// An empty ring.
    pub fn new() -> TraceRing {
        TraceRing
    }

    /// Starts a new instance, clearing the ring; returns its number.
    pub fn begin_instance(&mut self) -> u64 {
        unimplemented!("host::telemetry::TraceRing::begin_instance")
    }

    /// Pushes a record; returns its sequence number.
    pub fn push(&mut self, _time_ms: u64, _kind: TraceKind, _detail: serde_json::Value) -> u64 {
        unimplemented!("host::telemetry::TraceRing::push")
    }

    /// The records, oldest first.
    pub fn records(&self) -> Vec<TraceRecord> {
        unimplemented!("host::telemetry::TraceRing::records")
    }
}

/// Replaces every payload in a detail by its byte length, keeping the tags (DEV-672).
pub fn redact_detail(_detail: &serde_json::Value) -> serde_json::Value {
    unimplemented!("host::telemetry::redact_detail")
}

/// Whether `--unredacted` is allowed: only for a dev install (DEV-672).
pub fn check_unredacted(_dev_install: bool) -> Result<(), Refusal> {
    unimplemented!("host::telemetry::check_unredacted")
}

/// One node of the last rendered tree as the inspector shows it (DEV-680).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InspectNode {
    /// The widget kind.
    pub kind: String,
    /// The node key.
    pub key: Option<String>,
    /// The text, when any.
    pub text: Option<String>,
    /// The style summary, when any.
    pub style: Option<String>,
    /// Whether the node is selected.
    pub selected: bool,
    /// The children.
    pub children: Vec<InspectNode>,
}

/// Renders the tree inspector text of a view into `columns` by `rows` (DEV-680, DEV-681).
pub fn render_inspector(_view: &str, _tree: &InspectNode, _columns: usize, _rows: usize) -> String {
    unimplemented!("host::telemetry::render_inspector")
}

/// The memory figures of one side (DEV-685).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemoryView {
    /// The side.
    pub side: Side,
    /// Current linear memory in bytes.
    pub linear_bytes: u64,
    /// Peak linear memory in bytes.
    pub peak_bytes: u64,
    /// The memory limit in bytes.
    pub limit_bytes: u64,
    /// Storage used in bytes.
    pub storage_bytes: u64,
    /// Instances started.
    pub instances: u32,
    /// Restarts.
    pub restarts: u32,
}

/// Renders the memory view text of a plugin (DEV-685, DEV-686).
pub fn render_memory_view(_plugin: &str, _view: &MemoryView) -> String {
    unimplemented!("host::telemetry::render_memory_view")
}

/// Bytes as a binary-prefixed figure such as `9.8 MiB` (DEV-686).
pub fn format_binary(_bytes: u64) -> String {
    unimplemented!("host::telemetry::format_binary")
}

/// The timing summary of one export (DEV-690).
#[derive(Debug, Clone, PartialEq)]
pub struct TimingRecord {
    /// Calls recorded.
    pub calls: u64,
    /// Total milliseconds.
    pub total_ms: f64,
    /// Median, when any call was recorded.
    pub p50_ms: Option<f64>,
    /// 95th percentile, when any call was recorded.
    pub p95_ms: Option<f64>,
    /// The slowest call, when any.
    pub max_ms: Option<f64>,
    /// Calls over the export's budget.
    pub over_budget: u64,
}

/// A per-export call profiler (DEV-690, DEV-691).
#[derive(Debug, Default)]
pub struct Profiler;

impl Profiler {
    /// An empty profiler.
    pub fn new() -> Profiler {
        Profiler
    }

    /// Records one call of `export`.
    pub fn record(&mut self, _export: &str, _took: Duration) {
        unimplemented!("host::telemetry::Profiler::record")
    }

    /// The timing summary of `export`.
    pub fn timing(&self, _export: &str) -> TimingRecord {
        unimplemented!("host::telemetry::Profiler::timing")
    }

    /// The profiler's own overhead as a fraction of the measured time (DEV-691).
    pub fn overhead_fraction(&self) -> f64 {
        unimplemented!("host::telemetry::Profiler::overhead_fraction")
    }
}

// ---------------------------------------------------------------------------------------------
// Dev loop (reference-debugging-tools.md DEV-710..716).
// ---------------------------------------------------------------------------------------------

/// What one rebuild produced (DEV-713).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildOutcome {
    /// The build succeeded with this version.
    Built {
        /// The built version.
        version: String,
    },
    /// The build failed.
    Failed {
        /// The compiler errors.
        errors: Vec<String>,
    },
}

/// What the dev loop needs from the host (DEV-710..716).
pub trait DevBackend: Send + Sync {
    /// Whether a release install of `id` exists.
    fn release_installed(&self, id: &str) -> bool;
    /// Builds the project.
    fn build(&self) -> RebuildOutcome;
    /// Deactivates the running instance with a reason.
    fn deactivate(&self, reason: &str);
    /// Loads a built version.
    fn load(&self, version: &str);
    /// Activates the loaded version.
    fn activate(&self);
}

/// How a dev install shows up (DEV-710, DEV-711).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevInstall {
    /// The badge text, `[dev]`.
    pub badge: String,
    /// The plugin-list row label.
    pub list_label: String,
    /// The settings label.
    pub settings_label: String,
    /// The dev install's data folder.
    pub data_dir: PathBuf,
    /// The release install's data folder.
    pub release_data_dir: PathBuf,
}

/// What one rebuild did (DEV-712..714).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevReport {
    /// Whether the plugin was reloaded.
    pub reloaded: bool,
    /// Whether storage was kept across the reload.
    pub storage_kept: bool,
    /// The version now running.
    pub running_version: String,
    /// Build errors, when the build failed.
    pub errors: Vec<String>,
}

/// The rebuild-and-reload loop of `ilium plugin dev` (DEV-710..716).
pub struct DevLoop {
    running_version: Option<String>,
}

impl std::fmt::Debug for DevLoop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DevLoop")
            .field("running_version", &self.running_version)
            .finish()
    }
}

impl DevLoop {
    /// A loop over a backend.
    pub fn new(_backend: std::sync::Arc<dyn DevBackend>) -> DevLoop {
        DevLoop {
            running_version: None,
        }
    }

    /// Starts the dev install of `id`; refused with `dev.id-installed` when a release install
    /// exists and `replace_installed` is false (DEV-710, DEV-715).
    pub fn start(&mut self, _id: &str, _replace_installed: bool) -> Result<DevInstall, Refusal> {
        unimplemented!("host::telemetry::DevLoop::start")
    }

    /// Rebuilds and, on success, reloads (DEV-712..714).
    pub fn rebuild(&mut self) -> DevReport {
        unimplemented!("host::telemetry::DevLoop::rebuild")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_unredacted, cut_wasi_line, disabled_log, format_binary, format_local_time,
        format_log_line, host_line, redact_detail, render_inspector, render_memory_view,
        resolve_threshold, restart_log, truncate_message, why_lines, BreachKind, DevBackend,
        DevLoop, HealthInputs, HostLogLimiter, InspectNode, LogConfig, LogLevel, LogLine, LogRing,
        LogSide, MemoryView, NearMissTracker, PluginLog, Profiler, Reader, RebuildOutcome,
        SideTelemetry, SlowCallReporter, Span, SpanRecorder, Summary, TelemetryRegistry, TraceKind,
        TraceRing, COUNTER_NAMES, DURATION_BUCKET_BOUNDS_MS, LOG_MESSAGE_MAX_BYTES,
        LOG_ROTATE_BYTES, TELEMETRY_MAX_BYTES_PER_SIDE, TRACE_RING_CAPACITY,
    };
    use crate::calls::CallClass;
    use ilium_plugin_api::{Side, Tier};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    const HOUR_MS: u64 = 3_600_000;
    const MINUTE_MS: u64 = 60_000;

    fn log_line(plugin: &str, message: &str) -> LogLine {
        LogLine {
            plugin: plugin.to_string(),
            level: "info".to_string(),
            message: message.to_string(),
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ilium-telemetry-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test folder is created");
        dir
    }

    #[test]
    fn log_line_contains_matches_substring_and_empty_needle() {
        let line = log_line("example.notes", "storage near quota");
        assert!(line.contains("near quota"));
        assert!(line.contains(""));
        assert!(!line.contains("restart"));
    }

    #[test]
    fn log_ring_keeps_only_capacity_lines_oldest_dropped() {
        let mut ring = LogRing::new(2);
        ring.push(log_line("example.notes", "a1"));
        ring.push(log_line("example.other", "b1"));
        ring.push(log_line("example.notes", "a2"));
        let notes: Vec<String> = ring
            .lines_of("example.notes")
            .into_iter()
            .map(|line| line.message)
            .collect();
        assert_eq!(notes, vec!["a2".to_string()]);
        let other: Vec<String> = ring
            .lines_of("example.other")
            .into_iter()
            .map(|line| line.message)
            .collect();
        assert_eq!(other, vec!["b1".to_string()]);
    }

    #[test]
    fn log_ring_lines_of_returns_oldest_first_and_empty_for_unknown_plugin() {
        let mut ring = LogRing::new(4);
        ring.push(log_line("example.notes", "a1"));
        ring.push(log_line("example.notes", "a2"));
        ring.push(log_line("example.notes", "a3"));
        let messages: Vec<String> = ring
            .lines_of("example.notes")
            .into_iter()
            .map(|line| line.message)
            .collect();
        assert_eq!(
            messages,
            vec!["a1".to_string(), "a2".to_string(), "a3".to_string()]
        );
        assert!(ring.lines_of("example.unknown").is_empty());
        assert!(LogRing::new(4).lines_of("example.notes").is_empty());
    }

    #[test]
    fn cut_wasi_line_keeps_short_line_and_accepts_empty_input() {
        assert_eq!(cut_wasi_line("short line"), "short line");
        assert_eq!(cut_wasi_line(""), "");
    }

    /// REQ: HST-355
    #[test]
    fn near_miss_threshold_is_eighty_percent_of_deadline() {
        let mut tracker = NearMissTracker::new();
        assert!(tracker.record_call(0, 1_000, 800));
        assert!(!tracker.record_call(0, 1_000, 799));
        assert_eq!(tracker.count_in_window(0), 1);
    }

    /// REQ: HST-313
    #[test]
    fn near_miss_window_is_five_minutes() {
        let mut tracker = NearMissTracker::new();
        assert!(tracker.record_call(0, 100, 100));
        assert_eq!(tracker.count_in_window(1), 1);
        assert_eq!(tracker.count_in_window(5 * MINUTE_MS + 1), 0);
    }

    /// REQ: HST-354
    #[test]
    fn counter_names_are_the_sixteen_listed_in_order() {
        assert_eq!(COUNTER_NAMES.len(), 16);
        assert_eq!(COUNTER_NAMES[0], "calls");
        assert_eq!(COUNTER_NAMES[2], "near_misses");
        assert_eq!(COUNTER_NAMES[15], "failures");
    }

    /// REQ: HST-354
    #[test]
    fn duration_bucket_bounds_are_ascending_and_span_one_second() {
        assert_eq!(DURATION_BUCKET_BOUNDS_MS.len(), 12);
        assert_eq!(DURATION_BUCKET_BOUNDS_MS[0], 0.1);
        assert_eq!(DURATION_BUCKET_BOUNDS_MS[11], 1000.0);
        assert!(DURATION_BUCKET_BOUNDS_MS
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
    }

    /// REQ: HST-352
    #[test]
    fn per_side_memory_limit_is_sixty_four_kib() {
        assert_eq!(TELEMETRY_MAX_BYTES_PER_SIDE, 64 * 1024);
    }

    /// REQ: HST-354
    #[test]
    fn record_call_counts_calls_per_class() {
        let side = SideTelemetry::new();
        side.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(100),
            1_000,
        );
        side.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(100),
            1_000,
        );
        side.record_call(
            CallClass::Render,
            Duration::from_millis(1),
            Duration::from_millis(100),
            1_000,
        );
        let buckets = side.snapshot(1_000);
        let bucket = buckets.last().expect("the current hour has a bucket");
        assert_eq!(bucket.calls(CallClass::Event), 2);
        assert_eq!(bucket.calls(CallClass::Render), 1);
        assert_eq!(bucket.calls(CallClass::Command), 0);
    }

    /// REQ: HST-354
    #[test]
    fn duration_histogram_places_calls_in_bucket_bounds_and_overflow() {
        let side = SideTelemetry::new();
        side.record_call(
            CallClass::Event,
            Duration::from_micros(50),
            Duration::from_secs(5),
            1_000,
        );
        side.record_call(
            CallClass::Event,
            Duration::from_millis(150),
            Duration::from_secs(5),
            1_000,
        );
        side.record_call(
            CallClass::Event,
            Duration::from_millis(2000),
            Duration::from_secs(5),
            1_000,
        );
        let buckets = side.snapshot(1_000);
        let histogram = buckets
            .last()
            .expect("bucket")
            .duration_histogram(CallClass::Event);
        assert_eq!(histogram.len(), 13);
        assert_eq!(histogram[0], 1);
        assert_eq!(histogram[10], 1);
        assert_eq!(histogram[12], 1);
        assert_eq!(histogram.iter().sum::<u64>(), 3);
    }

    /// REQ: HST-355
    #[test]
    fn side_near_miss_counts_only_calls_at_eighty_percent_of_deadline() {
        let side = SideTelemetry::new();
        side.record_call(
            CallClass::Event,
            Duration::from_millis(90),
            Duration::from_millis(100),
            1_000,
        );
        side.record_call(
            CallClass::Event,
            Duration::from_millis(10),
            Duration::from_millis(100),
            1_000,
        );
        let buckets = side.snapshot(1_000);
        assert_eq!(
            buckets
                .last()
                .expect("bucket")
                .near_misses(CallClass::Event),
            1
        );
    }

    /// REQ: HST-354
    #[test]
    fn breaches_are_counted_separately_by_kind() {
        let side = SideTelemetry::new();
        side.record_breach(CallClass::Command, BreachKind::Hard, 1_000);
        side.record_breach(CallClass::Command, BreachKind::Soft, 1_000);
        side.record_breach(CallClass::Command, BreachKind::Soft, 1_000);
        let buckets = side.snapshot(1_000);
        let bucket = buckets.last().expect("bucket");
        assert_eq!(bucket.breaches(CallClass::Command, BreachKind::Hard), 1);
        assert_eq!(bucket.breaches(CallClass::Command, BreachKind::Soft), 2);
    }

    /// REQ: HST-354
    #[test]
    fn traps_queue_depth_and_events_are_recorded_per_key() {
        let side = SideTelemetry::new();
        side.record_trap("trap.deadline", 1_000);
        side.record_queue_depth(3, 1_000);
        side.record_queue_depth(7, 1_000);
        side.record_queue_depth(5, 1_000);
        side.record_event_dropped("ui", 2, 1_000);
        side.record_event_dropped("ui", 3, 1_000);
        side.record_event_coalesced("ui", 4, 1_000);
        let buckets = side.snapshot(1_000);
        let bucket = buckets.last().expect("bucket");
        assert_eq!(bucket.traps("trap.deadline"), 1);
        assert_eq!(bucket.traps("host.stack-overflow"), 0);
        assert_eq!(bucket.queue_depth_high_water(), 7);
        assert_eq!(bucket.events_dropped("ui"), 5);
        assert_eq!(bucket.events_dropped("other"), 0);
        assert_eq!(bucket.events_coalesced("ui"), 4);
    }

    /// REQ: HST-354
    #[test]
    fn quota_rate_limit_cpu_restart_and_failure_counters_accumulate() {
        let side = SideTelemetry::new();
        side.record_quota_rejection(1_000);
        side.record_rate_limited("fs.read", 1_000);
        side.record_rate_limited("fs.read", 1_000);
        side.record_cpu_ms(15, 1_000);
        side.record_cpu_ms(10, 1_000);
        side.record_restart("trap", 1_000);
        side.record_failure("helper-died", 1_000);
        let buckets = side.snapshot(1_000);
        let bucket = buckets.last().expect("bucket");
        assert_eq!(bucket.quota_rejections(), 1);
        assert_eq!(bucket.rate_limited("fs.read"), 2);
        assert_eq!(bucket.cpu_ms(), 25);
        assert_eq!(bucket.restarts("trap"), 1);
        assert_eq!(bucket.failures("helper-died"), 1);
    }

    /// REQ: HST-354
    #[test]
    fn memory_peak_keeps_the_highest_observation() {
        let side = SideTelemetry::new();
        side.record_memory(100, 1_000);
        side.record_memory(50, 1_000);
        let buckets = side.snapshot(1_000);
        assert_eq!(buckets.last().expect("bucket").memory_peak(), 100);
    }

    /// REQ: HST-357
    #[test]
    fn snapshot_keeps_twenty_four_hours_and_drops_older_buckets() {
        let side = SideTelemetry::new();
        side.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(100),
            0,
        );
        let now_ms = 25 * HOUR_MS;
        side.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(100),
            now_ms,
        );
        let buckets = side.snapshot(now_ms);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].hour_start_ms, now_ms);
    }

    /// REQ: HST-352
    #[test]
    fn memory_bytes_stays_within_per_side_limit_after_many_calls() {
        let side = SideTelemetry::new();
        for index in 0..100_000u64 {
            side.record_call(
                CallClass::Event,
                Duration::from_micros(index % 2_000),
                Duration::from_millis(100),
                index * 1_000,
            );
        }
        assert!(side.memory_bytes() <= TELEMETRY_MAX_BYTES_PER_SIDE);
    }

    /// REQ: HST-357
    #[test]
    fn end_instance_writes_one_summary_only_when_calls_happened() {
        let idle = SideTelemetry::new();
        assert!(idle.end_instance(1_000).is_none());

        let busy = SideTelemetry::new();
        busy.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(100),
            1_000,
        );
        assert!(busy.end_instance(2_000).is_some());
        assert!(busy.end_instance(2_000).is_none());
    }

    #[test]
    fn registry_side_returns_the_same_counters_for_the_same_plugin_and_side() {
        let registry = TelemetryRegistry::new();
        let first = registry.side("example.notes", Side::Server);
        let second = registry.side("example.notes", Side::Server);
        assert!(Arc::ptr_eq(&first, &second));
    }

    /// REQ: HST-356
    #[test]
    fn read_is_allowed_for_kernel_cli_and_ui_but_refused_for_other_plugins() {
        let registry = TelemetryRegistry::new();
        assert!(registry
            .read(&Reader::Kernel, "example.notes", Side::Server, 1_000)
            .is_ok());
        assert!(registry
            .read(&Reader::Cli, "example.notes", Side::Server, 1_000)
            .is_ok());
        assert!(registry
            .read(&Reader::Ui, "example.notes", Side::Server, 1_000)
            .is_ok());
        assert!(registry
            .read(
                &Reader::Plugin("example.other".to_string()),
                "example.notes",
                Side::Server,
                1_000,
            )
            .is_err());
    }

    /// REQ: HST-362
    #[test]
    fn plugin_info_is_free_for_self_and_needs_manage_for_others() {
        let registry = TelemetryRegistry::new();
        assert!(registry
            .plugin_info("example.notes", "example.notes", false, 1_000)
            .is_ok());
        assert!(registry
            .plugin_info("example.other", "example.notes", false, 1_000)
            .is_err());
        assert!(registry
            .plugin_info("example.other", "example.notes", true, 1_000)
            .is_ok());
    }

    #[test]
    fn summary_default_is_all_zero() {
        assert_eq!(
            Summary::default(),
            Summary {
                calls: 0,
                near_misses: 0,
            }
        );
    }

    /// REQ: HST-358
    #[test]
    fn host_line_builds_known_condition_and_refuses_unknown_or_empty_id() {
        let line = host_line(
            "example.notes",
            Side::Server,
            "trap.deadline",
            "soft breach",
        )
        .expect("trap.deadline is a known condition id");
        assert_eq!(line.plugin, "example.notes");
        assert_eq!(line.side, Side::Server);
        assert_eq!(line.id, "trap.deadline");
        assert_eq!(line.level, "warn");
        assert_eq!(line.detail, "soft breach");

        let trap = host_line("example.notes", Side::Client, "host.trap", "code").expect("known");
        assert_eq!(trap.level, "error");

        assert!(host_line("example.notes", Side::Client, "made.up", "x").is_err());
        assert!(host_line("example.notes", Side::Client, "", "x").is_err());
    }

    /// REQ: HST-358
    #[test]
    fn host_log_limiter_admits_twenty_lines_per_minute_per_side_and_kind() {
        let mut limiter = HostLogLimiter::new();
        let admitted = (0..20)
            .filter(|_| limiter.admit(Side::Server, "trap.deadline", 0))
            .count();
        assert_eq!(admitted, 20);
        assert!(!limiter.admit(Side::Server, "trap.deadline", 0));
        assert!(limiter.admit(Side::Client, "trap.deadline", 0));
        assert!(limiter.admit(Side::Server, "trap.deadline", 2 * MINUTE_MS));
    }

    /// REQ: HST-358
    #[test]
    fn host_log_limiter_reports_suppressed_count_once() {
        let mut limiter = HostLogLimiter::new();
        for _ in 0..21 {
            limiter.admit(Side::Server, "trap.deadline", 0);
        }
        assert!(limiter
            .take_suppressed_summary(Side::Server, "trap.deadline", MINUTE_MS + 1)
            .is_some_and(|text| text.contains("suppressed")));
        assert!(limiter
            .take_suppressed_summary(Side::Server, "trap.deadline", MINUTE_MS + 2)
            .is_none());
    }

    /// REQ: HST-359
    #[test]
    fn slow_call_report_needs_a_call_over_half_the_deadline() {
        let mut reporter = SlowCallReporter::new();
        reporter.record(
            Side::Server,
            CallClass::Event,
            Duration::from_millis(50),
            Duration::from_millis(100),
            0,
        );
        assert!(reporter.due_report(Side::Server, MINUTE_MS).is_none());
    }

    /// REQ: HST-359
    #[test]
    fn slow_call_report_lists_three_slowest_once_per_minute() {
        let mut reporter = SlowCallReporter::new();
        for millis in [60u64, 70, 80, 90] {
            reporter.record(
                Side::Server,
                CallClass::Event,
                Duration::from_millis(millis),
                Duration::from_millis(100),
                0,
            );
        }
        let report = reporter
            .due_report(Side::Server, MINUTE_MS)
            .expect("calls over half the deadline are reported");
        assert_eq!(report.id, "host.slow-calls");
        assert_eq!(report.top.len(), 3);
        assert_eq!(report.top[0].1, Duration::from_millis(90));
        assert_eq!(report.top[2].1, Duration::from_millis(70));
        assert!(reporter.due_report(Side::Server, MINUTE_MS + 1).is_none());
    }

    /// REQ: HST-360
    #[test]
    fn span_serializes_the_six_fields_with_micro_second_times() {
        let span = Span {
            plugin: "example.notes".to_string(),
            side: Side::Server,
            class: CallClass::Event,
            queue_time: Duration::from_micros(1500),
            run_time: Duration::from_micros(2500),
            forwarded_imports: 2,
        };
        let value = serde_json::to_value(&span).expect("span serializes");
        for name in Span::FIELD_NAMES {
            assert!(value.get(name).is_some(), "missing field {name}");
        }
        assert_eq!(value["plugin"].as_str(), Some("example.notes"));
        assert_eq!(value["class"].as_str(), Some("event"));
        assert_eq!(value["queue_time_us"].as_u64(), Some(1500));
        assert_eq!(value["run_time_us"].as_u64(), Some(2500));
        assert_eq!(value["forwarded_imports"].as_u64(), Some(2));
    }

    /// REQ: HST-360
    #[test]
    fn span_field_names_are_in_documented_order() {
        assert_eq!(Span::FIELD_NAMES.len(), 6);
        assert_eq!(Span::FIELD_NAMES[0], "plugin");
        assert_eq!(Span::FIELD_NAMES[3], "queue_time_us");
        assert_eq!(Span::FIELD_NAMES[5], "forwarded_imports");
    }

    /// REQ: HST-360
    #[test]
    fn span_recorder_is_off_unless_enabled_and_then_keeps_spans_in_order() {
        assert!(!SpanRecorder::from_env(None).is_enabled());
        assert!(!SpanRecorder::from_env(Some("0")).is_enabled());
        assert!(!SpanRecorder::from_env(Some("")).is_enabled());
        assert!(SpanRecorder::from_env(Some("1")).is_enabled());

        let span = |run_micros: u64| Span {
            plugin: "example.notes".to_string(),
            side: Side::Scene,
            class: CallClass::Frame,
            queue_time: Duration::ZERO,
            run_time: Duration::from_micros(run_micros),
            forwarded_imports: 0,
        };

        let mut disabled = SpanRecorder::from_env(None);
        disabled.record(span(10));
        assert!(disabled.spans().is_empty());

        let mut enabled = SpanRecorder::from_env(Some("1"));
        enabled.record(span(10));
        enabled.record(span(20));
        assert_eq!(enabled.spans().len(), 2);
        assert_eq!(enabled.spans()[0].run_time, Duration::from_micros(10));
        assert_eq!(enabled.spans()[1].run_time, Duration::from_micros(20));
    }

    /// REQ: HST-363
    #[test]
    fn why_lines_are_empty_without_firing_signals() {
        assert!(why_lines(&HealthInputs::default()).is_empty());
        let below_memory_line = HealthInputs {
            memory_peak_bytes: 800,
            memory_limit_bytes: 1000,
            ..Default::default()
        };
        assert!(why_lines(&below_memory_line).is_empty());
    }

    /// REQ: HST-363
    #[test]
    fn why_lines_report_memory_near_limit_when_over_eighty_percent() {
        let near_limit = HealthInputs {
            memory_peak_bytes: 900,
            memory_limit_bytes: 1000,
            ..Default::default()
        };
        assert!(!why_lines(&near_limit).is_empty());
    }

    /// REQ: DEV-652
    #[test]
    fn log_levels_have_words_and_order_from_trace_to_error() {
        assert_eq!(LogLevel::ALL.len(), 5);
        assert!(LogLevel::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(LogLevel::Trace.as_str(), "trace");
        assert_eq!(LogLevel::Debug.as_str(), "debug");
        assert_eq!(LogLevel::Info.as_str(), "info");
        assert_eq!(LogLevel::Warn.as_str(), "warn");
        assert_eq!(LogLevel::Error.as_str(), "error");
        assert!(LogLevel::Warn > LogLevel::Info);
    }

    /// REQ: DEV-650
    #[test]
    fn log_side_words_include_host() {
        assert_eq!(LogSide::Server.as_str(), "server");
        assert_eq!(LogSide::Client.as_str(), "client");
        assert_eq!(LogSide::Scene.as_str(), "scene");
        assert_eq!(LogSide::Host.as_str(), "host");
    }

    /// REQ: DEV-650
    #[test]
    fn plugin_log_line_has_fixed_columns_for_plugin_side() {
        let line = format_log_line(
            "2030-01-01 09:14:02",
            LogLevel::Info,
            LogSide::Server,
            "example.notes",
            "activate: ready",
        );
        assert_eq!(
            line,
            "2030-01-01 09:14:02 info  server example.notes activate: ready"
        );
    }

    /// REQ: DEV-650
    #[test]
    fn host_log_line_pads_side_column_to_six_characters() {
        let line = format_log_line(
            "2030-01-01 09:15:40",
            LogLevel::Error,
            LogSide::Host,
            "example.notes",
            "restarting in 1 s (failure 1 of 3 in 10 minutes)",
        );
        assert_eq!(
            line,
            "2030-01-01 09:15:40 error host   example.notes restarting in 1 s (failure 1 of 3 in 10 minutes)"
        );
    }

    /// REQ: DEV-650
    #[test]
    fn log_line_writes_newline_in_message_as_escape() {
        let line = format_log_line(
            "2030-01-01 09:14:02",
            LogLevel::Info,
            LogSide::Server,
            "example.notes",
            "first\nsecond",
        );
        assert!(!line.contains('\n'));
        assert!(line.contains("first\\nsecond"));
    }

    /// REQ: DEV-650
    #[test]
    fn local_time_column_is_a_european_timestamp() {
        let text = format_local_time(SystemTime::UNIX_EPOCH);
        assert_eq!(text.len(), 19);
        assert_eq!(text.as_bytes()[4], b'-');
        assert_eq!(text.as_bytes()[7], b'-');
        assert_eq!(text.as_bytes()[10], b' ');
        assert_eq!(text.as_bytes()[13], b':');
        assert_eq!(text.as_bytes()[16], b':');
    }

    /// REQ: DEV-651
    #[test]
    fn message_at_exactly_the_limit_is_kept_unchanged() {
        let exact = "a".repeat(LOG_MESSAGE_MAX_BYTES);
        assert_eq!(truncate_message(&exact), exact);
        assert_eq!(truncate_message(""), "");
    }

    /// REQ: DEV-651
    #[test]
    fn message_over_the_limit_is_cut_and_marked_truncated() {
        let long = "a".repeat(LOG_MESSAGE_MAX_BYTES + 1);
        let cut = truncate_message(&long);
        assert!(cut.ends_with("…[truncated]"));
        assert!(cut.len() <= LOG_MESSAGE_MAX_BYTES);
    }

    /// REQ: DEV-651
    #[test]
    fn truncation_keeps_multi_byte_characters_whole() {
        let wide = "é".repeat(10_000);
        let cut = truncate_message(&wide);
        assert!(cut.ends_with("…[truncated]"));
        assert!(cut.len() <= LOG_MESSAGE_MAX_BYTES);
    }

    /// REQ: DEV-660
    #[test]
    fn rotation_size_is_four_mebibytes_and_message_limit_is_sixteen_kib() {
        assert_eq!(LOG_ROTATE_BYTES, 4 * 1024 * 1024);
        assert_eq!(LOG_MESSAGE_MAX_BYTES, 16_384);
    }

    /// REQ: DEV-653
    #[test]
    fn dev_install_logs_at_debug_and_tiers_get_their_rate_budget() {
        assert_eq!(LogConfig::for_dev_install().threshold, LogLevel::Debug);
        assert_eq!(LogConfig::for_tier(Tier::T2).threshold, LogLevel::Info);
        assert_eq!(LogConfig::for_tier(Tier::T1).rate_per_second, 1000);
        assert_eq!(LogConfig::for_tier(Tier::T2).rate_per_second, 200);
    }

    /// REQ: DEV-654
    #[test]
    fn threshold_flag_beats_setting_which_beats_dev_default() {
        assert_eq!(
            resolve_threshold(Some(LogLevel::Error), Some("trace"), false),
            LogLevel::Error
        );
        assert_eq!(
            resolve_threshold(Some(LogLevel::Trace), None, true),
            LogLevel::Trace
        );
        assert_eq!(resolve_threshold(None, Some("warn"), false), LogLevel::Warn);
        assert_eq!(resolve_threshold(None, None, true), LogLevel::Debug);
        assert_eq!(resolve_threshold(None, None, false), LogLevel::Info);
    }

    /// REQ: DEV-655
    #[test]
    fn restart_line_names_delay_and_failure_count() {
        assert_eq!(
            restart_log(1, 1, 3),
            (
                LogLevel::Warn,
                "restarting in 1 s (failure 1 of 3 in 10 minutes)".to_string(),
            )
        );
        assert_eq!(
            restart_log(5, 2, 3).1,
            "restarting in 5 s (failure 2 of 3 in 10 minutes)"
        );
    }

    /// REQ: DEV-655
    #[test]
    fn disabled_line_names_the_failure_count() {
        assert_eq!(disabled_log(3).1, "disabled after 3 failures in 10 minutes");
    }

    /// REQ: DEV-653
    #[test]
    fn plugin_log_drops_lines_below_threshold_and_writes_lines_at_it() {
        let dir = test_dir("threshold");
        let mut log = PluginLog::open(&dir, "example.notes", LogConfig::default())
            .expect("the log opens in a fresh folder");
        assert!(log.write_host(
            "2030-01-01 09:14:02",
            1_000,
            LogLevel::Info,
            "instance started"
        ));
        assert!(!log.write_host("2030-01-01 09:14:02", 1_000, LogLevel::Debug, "dropped"));
        assert!(!log.write_host("2030-01-01 09:14:02", 1_000, LogLevel::Trace, "dropped"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ: DEV-656
    #[test]
    fn plugin_log_rate_limits_lines_per_second() {
        let dir = test_dir("rate");
        let config = LogConfig {
            threshold: LogLevel::Info,
            rate_per_second: 2,
        };
        let mut log = PluginLog::open(&dir, "example.notes", config).expect("the log opens");
        assert!(log.write_plugin(
            "2030-01-01 09:14:02",
            1_000,
            Side::Server,
            LogLevel::Info,
            "one"
        ));
        assert!(log.write_plugin(
            "2030-01-01 09:14:02",
            1_000,
            Side::Server,
            LogLevel::Info,
            "two"
        ));
        assert!(!log.write_plugin(
            "2030-01-01 09:14:02",
            1_000,
            Side::Server,
            LogLevel::Info,
            "three"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ: DEV-672
    #[test]
    fn redaction_replaces_payload_with_byte_count() {
        let detail = serde_json::json!({ "payload": "hello" });
        let text = redact_detail(&detail).to_string();
        assert!(!text.contains("hello"));
        assert!(text.contains("<redacted 5 bytes>"));
    }

    /// REQ: DEV-672
    #[test]
    fn empty_payload_redacts_to_zero_bytes() {
        let detail = serde_json::json!({ "payload": "" });
        assert!(redact_detail(&detail)
            .to_string()
            .contains("<redacted 0 bytes>"));
    }

    /// REQ: DEV-673
    #[test]
    fn unredacted_trace_is_allowed_only_for_dev_installs() {
        assert!(check_unredacted(true).is_ok());
        match check_unredacted(false) {
            Err(refusal) => assert_eq!(refusal.code, "trace.unredacted-denied"),
            Ok(()) => panic!("unredacted trace must be refused outside dev installs"),
        }
    }

    #[test]
    fn trace_kind_words_are_kebab_case() {
        assert_eq!(TraceKind::Event.as_str(), "event");
        assert_eq!(TraceKind::HostCall.as_str(), "host-call");
        assert_eq!(TraceKind::Lifecycle.as_str(), "lifecycle");
    }

    /// REQ: DEV-670
    #[test]
    fn trace_ring_holds_capacity_records_and_drops_the_oldest() {
        let mut ring = TraceRing::new();
        ring.begin_instance();
        let mut last_seq = 0u64;
        let total = TRACE_RING_CAPACITY as u64 + 6;
        for time_ms in 0..total {
            last_seq = ring.push(time_ms, TraceKind::Event, serde_json::json!({}));
        }
        let records = ring.records();
        assert_eq!(records.len(), TRACE_RING_CAPACITY);
        assert_eq!(records.last().map(|record| record.seq), Some(last_seq));
        assert_eq!(
            records.first().map(|record| record.seq),
            Some(last_seq - (TRACE_RING_CAPACITY as u64 - 1))
        );
    }

    /// REQ: DEV-676
    #[test]
    fn trace_ring_keeps_earlier_instances_marked_by_number() {
        let mut ring = TraceRing::new();
        let first_instance = ring.begin_instance();
        ring.push(1, TraceKind::Event, serde_json::json!({}));
        let second_instance = ring.begin_instance();
        ring.push(2, TraceKind::Render, serde_json::json!({}));
        assert_ne!(first_instance, second_instance);
        let records = ring.records();
        assert!(records
            .iter()
            .any(|record| record.instance == first_instance));
        assert!(records
            .iter()
            .any(|record| record.instance == second_instance));
    }

    /// REQ: DEV-681
    #[test]
    fn inspector_shows_kind_key_text_and_style_and_marks_selection() {
        let node = InspectNode {
            kind: "button".to_string(),
            key: Some("save".to_string()),
            text: Some("Save".to_string()),
            style: Some("primary".to_string()),
            selected: false,
            children: Vec::new(),
        };
        let output = render_inspector("main", &node, 80, 24);
        for part in ["button", "save", "Save", "primary"] {
            assert!(output.contains(part), "missing {part} in inspector output");
        }
        let selected = InspectNode {
            selected: true,
            ..node.clone()
        };
        assert_ne!(render_inspector("main", &selected, 80, 24), output);
    }

    /// REQ: DEV-701
    #[test]
    fn memory_view_warns_only_when_peak_is_over_eighty_percent() {
        let mut view = MemoryView {
            side: Side::Server,
            linear_bytes: 0,
            peak_bytes: 900,
            limit_bytes: 1000,
            storage_bytes: 0,
            instances: 1,
            restarts: 0,
        };
        assert!(render_memory_view("example.notes", &view)
            .contains("warning: peak above 80 percent of memory_mib"));
        view.peak_bytes = 800;
        assert!(!render_memory_view("example.notes", &view)
            .contains("warning: peak above 80 percent of memory_mib"));
    }

    /// REQ: DEV-702
    #[test]
    fn binary_figures_use_binary_prefixes() {
        assert_eq!(format_binary(1024), "1.0 KiB");
        assert_eq!(format_binary(1_048_576), "1.0 MiB");
        assert_eq!(format_binary(10_276_045), "9.8 MiB");
    }

    /// REQ: DEV-692
    #[test]
    fn export_without_calls_reports_zero_calls_and_no_percentiles() {
        let profiler = Profiler::new();
        let timing = profiler.timing("render");
        assert_eq!(timing.calls, 0);
        assert!(timing.p50_ms.is_none());
        assert!(timing.p95_ms.is_none());
        assert!(timing.max_ms.is_none());
    }

    /// REQ: DEV-691
    #[test]
    fn profiler_samples_every_call_and_summarises_the_export() {
        let mut profiler = Profiler::new();
        for millis in 1..=100u64 {
            profiler.record("on-events", Duration::from_millis(millis));
        }
        let timing = profiler.timing("on-events");
        assert_eq!(timing.calls, 100);
        assert!((timing.total_ms - 5050.0).abs() < 1e-6);
        let median = timing
            .p50_ms
            .expect("median exists when calls were recorded");
        assert!((49.0..=51.0).contains(&median));
        let slowest = timing.max_ms.expect("slowest call exists");
        assert!((slowest - 100.0).abs() < 1e-6);
        assert_eq!(timing.over_budget, 80);
    }

    /// REQ: DEV-691
    #[test]
    fn render_budget_is_ten_milliseconds() {
        let mut profiler = Profiler::new();
        profiler.record("render", Duration::from_millis(5));
        profiler.record("render", Duration::from_millis(12));
        assert_eq!(profiler.timing("render").over_budget, 1);
    }

    /// REQ: DEV-693
    #[test]
    fn profiler_overhead_stays_within_five_percent() {
        let mut profiler = Profiler::new();
        for _ in 0..1000 {
            profiler.record("on-events", Duration::from_millis(1));
        }
        let overhead = profiler.overhead_fraction();
        assert!((0.0..=0.05).contains(&overhead));
    }

    struct FakeBackend {
        has_release: bool,
        outcome: RebuildOutcome,
    }

    impl DevBackend for FakeBackend {
        fn release_installed(&self, _id: &str) -> bool {
            self.has_release
        }

        fn build(&self) -> RebuildOutcome {
            self.outcome.clone()
        }

        fn deactivate(&self, _reason: &str) {}

        fn load(&self, _version: &str) {}

        fn activate(&self) {}
    }

    fn backend_with(has_release: bool, outcome: RebuildOutcome) -> Arc<dyn DevBackend> {
        Arc::new(FakeBackend {
            has_release,
            outcome,
        })
    }

    /// REQ: DEV-711
    #[test]
    fn dev_start_is_refused_when_release_is_installed_without_replace() {
        let backend = backend_with(
            true,
            RebuildOutcome::Built {
                version: "0.2.0".to_string(),
            },
        );
        let mut dev_loop = DevLoop::new(backend);
        match dev_loop.start("example.notes", false) {
            Err(refusal) => assert_eq!(refusal.code, "dev.id-installed"),
            Ok(_) => panic!("dev start must refuse when the release is installed"),
        }
    }

    /// REQ: DEV-710
    #[test]
    fn dev_start_with_replace_shows_badge_and_separate_data_folder() {
        let backend = backend_with(
            true,
            RebuildOutcome::Built {
                version: "0.2.0".to_string(),
            },
        );
        let mut dev_loop = DevLoop::new(backend);
        let install = dev_loop
            .start("example.notes", true)
            .expect("replace_installed allows the dev install");
        assert_eq!(install.badge, "[dev]");
        assert_ne!(install.data_dir, install.release_data_dir);
    }

    /// REQ: DEV-712
    #[test]
    fn successful_rebuild_reloads_and_keeps_storage() {
        let backend = backend_with(
            false,
            RebuildOutcome::Built {
                version: "0.2.0".to_string(),
            },
        );
        let mut dev_loop = DevLoop::new(backend);
        dev_loop
            .start("example.notes", false)
            .expect("no release installed, so the dev install starts");
        let report = dev_loop.rebuild();
        assert!(report.reloaded);
        assert!(report.storage_kept);
        assert_eq!(report.running_version, "0.2.0");
        assert!(report.errors.is_empty());
    }

    /// REQ: DEV-713
    #[test]
    fn failed_rebuild_does_not_reload_and_prints_build_errors() {
        let backend = backend_with(
            false,
            RebuildOutcome::Failed {
                errors: vec!["unknown type".to_string()],
            },
        );
        let mut dev_loop = DevLoop::new(backend);
        dev_loop
            .start("example.notes", false)
            .expect("no release installed, so the dev install starts");
        let report = dev_loop.rebuild();
        assert!(!report.reloaded);
        assert_eq!(report.errors, vec!["unknown type".to_string()]);
    }
}
