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
    /// The stable condition id, such as `host.deadline`.
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
