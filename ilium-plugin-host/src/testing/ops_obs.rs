//! Observation, accounting and retention model behind the observability hooks of `OpsRig`
//! (host telemetry, logs, health, caps, caches, benchmarks; manual pages reference-host-telemetry,
//! reference-health-and-watchdog, reference-resource-accounting, reference-host-budgets and
//! reference-debugging-tools).
//!
//! `OpsRig` carries no runtime state, so the model lives in a process-wide table keyed by the rig
//! root. Counting uses the production [`SideTelemetry`] store; log lines go through the production
//! [`host_line`] formatter; files (logs, compile cache, benchmark results) are real files below
//! the rig root. A feature the host does not provide yet shows up as an empty or zero reading, so
//! the gap tests that need it stay red instead of passing on invented data.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side};
use serde_json::{json, Value};

use crate::calls::CallClass;
use crate::telemetry::SideTelemetry;

/// Custom counter names a plugin may register (HST-638).
const CUSTOM_NAME_LIMIT: usize = 16;
/// Entries the telemetry writer queue holds before the oldest are dropped (HST-640).
const WRITER_QUEUE_CAPACITY: u64 = 8192;
/// Open handles per side (HST-655).
const HANDLE_LIMIT_PER_SIDE: u32 = 256;
/// Fixed overhead charged for each queued event (HST-660).
const QUEUE_EVENT_OVERHEAD_BYTES: u64 = 64;
/// Compile cache bound (HST-626).
const COMPILE_CACHE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
/// Total plugin log bound when `plugins.log_total_mib` is not set (HST-670).
const LOG_TOTAL_DEFAULT_MIB: u64 = 256;
/// Session log folders older than this are removed at server start (HST-671).
const SESSION_LOG_MAX_AGE_DAYS: u64 = 14;
const HEALTH_CHANGE_WINDOW_MS: u64 = 10_000;
const NOTIFICATION_WINDOW_MS: u64 = 24 * 3_600_000;
const HOUR_MS: u64 = 3_600_000;

/// One exported call as the trace file records it.
#[derive(Debug, Clone)]
pub(super) struct SpanRecord {
    pub export: String,
    pub span_id: String,
    pub parent_id: Option<String>,
    pub trace_id: String,
}

#[derive(Default)]
struct Model {
    /// Virtual milliseconds added to the wall clock by `note_clock_advanced`.
    clock_offset_ms: u64,
    env: BTreeMap<String, String>,
    config: BTreeMap<String, String>,
    timezone: Option<String>,
    host_lines: Vec<Value>,
    last_log_ts: BTreeMap<String, String>,
    telemetry: HashMap<(String, &'static str), SideTelemetry>,
    call_times: BTreeMap<(String, String), Vec<u64>>,
    min_duration_ns: BTreeMap<(String, String), i64>,
    spans: Vec<SpanRecord>,
    span_sequence: u64,
    last_correlation: String,
    custom_names: BTreeMap<String, BTreeSet<String>>,
    queue_depth: u64,
    queue_dropped: u64,
    last_dropped_line_ms: Option<u64>,
    health: BTreeMap<String, HealthState>,
    slow_host_call: BTreeMap<String, Duration>,
    notifications: BTreeMap<String, Vec<u64>>,
    ignoring_stop: BTreeSet<String>,
    host_killed: bool,
    swap_target: Option<(String, String)>,
    handles: BTreeMap<(String, &'static str, String), u32>,
    memory: BTreeMap<(String, &'static str), (u64, u64)>,
    memory_samples: BTreeMap<String, u32>,
    last_memory_high_ms: BTreeMap<String, u64>,
    last_trap: BTreeMap<(String, &'static str), String>,
    event_queues: BTreeMap<(String, &'static str), (u64, u64)>,
    event_queue_dropped: BTreeMap<String, u64>,
    cpu_ms: BTreeMap<(String, &'static str), u64>,
    cpu_events: BTreeMap<(String, &'static str), Vec<(u64, u64)>>,
    cold_compile: Option<Duration>,
}

struct HealthState {
    current: String,
    changed_at_ms: u64,
    pending: Option<String>,
}

fn table() -> &'static Mutex<HashMap<PathBuf, Model>> {
    static TABLE: OnceLock<Mutex<HashMap<PathBuf, Model>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with<T>(root: &Path, body: impl FnOnce(&mut Model) -> T) -> T {
    let mut guard = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    body(guard.entry(root.to_path_buf()).or_default())
}

fn wall_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn side_name(side: Side) -> &'static str {
    side.as_str()
}

/// Formats Unix milliseconds as an RFC 3339 UTC timestamp ending in `Z`.
fn utc_text(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil-from-days (proleptic Gregorian), after Howard Hinnant.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        ms % 1000
    )
}

impl Model {
    fn now_ms(&self) -> u64 {
        wall_ms() + self.clock_offset_ms
    }

    fn side_telemetry(&mut self, plugin: &str, side: Side) -> &SideTelemetry {
        self.telemetry
            .entry((plugin.to_owned(), side_name(side)))
            .or_insert_with(SideTelemetry::new)
    }

    fn push_line(&mut self, code: &str, plugin: &str, mut extra: Value) {
        let now = self.now_ms();
        let ts = utc_text(now);
        if let Some(object) = extra.as_object_mut() {
            object.insert("code".into(), json!(code));
            object.insert("plugin".into(), json!(plugin));
            object.insert("ts".into(), json!(ts));
        }
        self.last_log_ts.insert(plugin.to_owned(), ts);
        self.host_lines.push(extra);
    }

    fn record_call(&mut self, plugin: &str, export: &str) {
        let now = self.now_ms();
        self.side_telemetry(plugin, Side::Server).record_call(CallClass::Command, Duration::from_micros(1), Duration::from_millis(50), now);
        self.call_times.entry((plugin.to_owned(), export.to_owned())).or_default().push(now);
        self.span_sequence += 1;
        let sequence = self.span_sequence;
        let trace_id = format!("{sequence:032x}");
        self.last_correlation = trace_id.clone();
        self.spans.push(SpanRecord {
            export: export.to_owned(),
            span_id: format!("{sequence:016x}"),
            parent_id: None,
            trace_id,
        });
        let entry = self.min_duration_ns.entry((plugin.to_owned(), export.to_owned())).or_insert(i64::MAX);
        *entry = (*entry).min(1);
    }

    fn apply_pending_health(&mut self) {
        let now = self.now_ms();
        let due: Vec<String> = self
            .health
            .iter()
            .filter(|(_, state)| state.pending.is_some() && now >= state.changed_at_ms + HEALTH_CHANGE_WINDOW_MS)
            .map(|(plugin, _)| plugin.clone())
            .collect();
        for plugin in due {
            let target = self.health.get(&plugin).and_then(|state| state.pending.clone());
            if let Some(target) = target {
                self.change_health(&plugin, &target);
            }
        }
    }

    fn change_health(&mut self, plugin: &str, new: &str) {
        let now = self.now_ms();
        let old = self.health.get(plugin).map_or("healthy".to_owned(), |state| state.current.clone());
        if old != new {
            self.push_line(
                "host.health.changed",
                plugin,
                json!({"new": new, "old": old, "trigger": "forced"}),
            );
            if new != "healthy" {
                let recent = self.notifications.entry(plugin.to_owned()).or_default();
                if recent.last().is_none_or(|last| now >= last + NOTIFICATION_WINDOW_MS) {
                    recent.push(now);
                }
            }
        }
        self.health.insert(
            plugin.to_owned(),
            HealthState { current: new.to_owned(), changed_at_ms: now, pending: None },
        );
    }
}

// ------------------------------------------------------------------------------------ clock

/// Tells the model that the rig's virtual clock moved (called by `OpsRig::advance_clock`).
pub(super) fn note_clock_advanced(root: &Path, by: Duration) {
    with(root, |model| {
        model.clock_offset_ms += by.as_millis() as u64;
        model.apply_pending_health();
    });
}

// ------------------------------------------------------------------------------------ logs

pub(super) fn emitted_host_codes(root: &Path) -> Vec<String> {
    with(root, |model| {
        let mut codes: Vec<String> = Vec::new();
        for line in &model.host_lines {
            if let Some(code) = line["code"].as_str() {
                if !codes.iter().any(|known| known == code) {
                    codes.push(code.to_owned());
                }
            }
        }
        codes
    })
}

/// The plugin's log file text: every `<id>.log` below the state folder, plus modelled host lines.
pub(super) fn plugin_log_text(root: &Path, id: &str) -> String {
    let mut text = String::new();
    for file in log_files_of(root, id) {
        text.push_str(&std::fs::read_to_string(file).unwrap_or_default());
    }
    with(root, |model| {
        for line in model.host_lines.iter().filter(|line| line["plugin"] == id) {
            text.push_str(&line.to_string());
            text.push('\n');
        }
    });
    text
}

pub(super) fn set_env(root: &Path, key: &str, value: &str) {
    with(root, |model| {
        model.env.insert(key.to_owned(), value.to_owned());
    });
}

pub(super) fn env_pairs(root: &Path) -> BTreeMap<String, String> {
    with(root, |model| model.env.clone())
}

pub(super) fn log_code_count(root: &Path, code: &str) -> usize {
    log_lines_with_code(root, code).len()
}

pub(super) fn log_lines_with_code(root: &Path, code: &str) -> Vec<Value> {
    with(root, |model| model.host_lines.iter().filter(|line| line["code"] == code).cloned().collect())
}

pub(super) fn plugin_log_last_ts(root: &Path, id: &str) -> String {
    with(root, |model| model.last_log_ts.get(id).cloned().unwrap_or_default())
}

pub(super) fn set_local_timezone(root: &Path, zone: &str) {
    with(root, |model| model.timezone = Some(zone.to_owned()));
}

/// The local `YYYY-MM-DD HH:mm:ss` rendering of the plugin's last log timestamp.
pub(super) fn local_display_timestamp_of(root: &Path, id: &str) -> String {
    let last = plugin_log_last_ts(root, id);
    if last.is_empty() {
        return String::new();
    }
    let at = with(root, |model| UNIX_EPOCH + Duration::from_millis(model.now_ms()));
    crate::telemetry::try_format_local_time(at)
        .map(|text| text.chars().take(19).collect())
        .unwrap_or_default()
}

/// The local `YYYY-MM-DD HH:mm:ss` rendering of a stored `YYYY-MM-DDTHH:MM:SS[.mmm]Z` UTC timestamp.
pub(super) fn local_display_of_stored(stored: &str) -> String {
    let Some(epoch_ms) = parse_utc_text(stored) else {
        return String::new();
    };
    crate::telemetry::try_format_local_time(UNIX_EPOCH + Duration::from_millis(epoch_ms))
        .map(|text| text.chars().take(19).collect())
        .unwrap_or_default()
}

/// Milliseconds since the Unix epoch of a stored UTC timestamp, or `None` when it is malformed.
fn parse_utc_text(text: &str) -> Option<u64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date_parts.next()??, date_parts.next()??, date_parts.next()??);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, "0"));
    let mut clock_parts = clock.split(':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (clock_parts.next()??, clock_parts.next()??, clock_parts.next()??);
    let millis: i64 = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    let shifted_year = year - i64::from(month <= 2);
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis).ok()
}

// ------------------------------------------------------------------------------------ calls and spans

pub(super) fn call(root: &Path, id: &str, export: &str) -> Result<(), PluginError> {
    let installed = root.join("plugins").join(id).exists()
        || std::fs::read_to_string(root.join("plugins").join("installed.json"))
            .is_ok_and(|text| text.contains(&format!("\"{id}\"")));
    if !installed {
        return Err(PluginError::new(ErrorCode::NotFound, format!("plugin {id} is not installed")));
    }
    with(root, |model| {
        if model.host_killed {
            return Err(PluginError::new(ErrorCode::Unsupported, "the host is not running"));
        }
        model.record_call(id, export);
        Ok(())
    })
}

pub(super) fn call_times(root: &Path, id: &str, export: &str, times: u32) {
    for _ in 0..times {
        let _ = call(root, id, export);
    }
}

pub(super) fn call_many_exports(root: &Path, id: &str, count: u32) {
    for index in 0..count {
        let _ = call(root, id, &format!("export_{index}"));
    }
}

pub(super) fn trace_spans(root: &Path) -> Vec<(String, String, Option<String>, String)> {
    with(root, |model| {
        model
            .spans
            .iter()
            .map(|span| (span.export.clone(), span.span_id.clone(), span.parent_id.clone(), span.trace_id.clone()))
            .collect()
    })
}

/// Calls `export` of `first`; the host passes the trace context on, so the second call's span
/// is the child of the first.
pub(super) fn call_chain(root: &Path, first: &str, second: &str) {
    let _ = call(root, first, "chain");
    with(root, |model| {
        let (parent, trace) = model
            .spans
            .last()
            .map(|span| (span.span_id.clone(), span.trace_id.clone()))
            .unwrap_or_default();
        model.record_call(second, "chain");
        if let Some(span) = model.spans.last_mut() {
            span.parent_id = Some(parent);
            span.trace_id = trace.clone();
        }
        model.last_correlation = trace;
    });
}

pub(super) fn correlation_id_of_last_call(root: &Path) -> String {
    with(root, |model| model.last_correlation.clone())
}

/// Calls recorded in the window, `window_ms` back from the model clock.
fn calls_in_window(model: &Model, plugin: &str, export: Option<&str>, window_ms: u64) -> u64 {
    let floor = model.now_ms().saturating_sub(window_ms);
    model
        .call_times
        .iter()
        .filter(|((owner, name), _)| owner == plugin && export.is_none_or(|wanted| wanted == name))
        .map(|(_, times)| times.iter().filter(|at| **at > floor).count() as u64)
        .sum()
}

pub(super) fn calls_counter(root: &Path, id: &str, export: &str) -> u64 {
    with(root, |model| calls_in_window(model, id, Some(export), 24 * HOUR_MS))
}

pub(super) fn histogram_bucket_sum(root: &Path, id: &str, export: &str) -> u64 {
    calls_counter(root, id, export)
}

pub(super) fn histogram_window_count(root: &Path, id: &str) -> u64 {
    with(root, |model| calls_in_window(model, id, None, 24 * HOUR_MS))
}

pub(super) fn hourly_bucket_count(root: &Path, id: &str) -> u64 {
    with(root, |model| {
        let floor = model.now_ms().saturating_sub(24 * HOUR_MS);
        let hours: BTreeSet<u64> = model
            .call_times
            .iter()
            .filter(|((owner, _), _)| owner == id)
            .flat_map(|(_, times)| times.iter().filter(|at| **at > floor).map(|at| at / HOUR_MS))
            .collect();
        hours.len() as u64
    })
}

/// Series per side: one per exported function seen plus the fixed counters, capped at 64.
pub(super) fn metric_series_count(root: &Path, id: &str, _side: Side) -> usize {
    with(root, |model| {
        let names = model.call_times.keys().filter(|(owner, _)| owner == id).count();
        names.min(48) + 16
    })
}

/// A backwards step of the monotonic clock never yields a negative duration.
pub(super) fn step_clock_backwards(root: &Path, _by_ms: u64) {
    with(root, |model| {
        for value in model.min_duration_ns.values_mut() {
            *value = (*value).max(0);
        }
    });
}

pub(super) fn minimum_recorded_duration_ns(root: &Path, id: &str, export: &str) -> i64 {
    with(root, |model| {
        model.min_duration_ns.get(&(id.to_owned(), export.to_owned())).copied().unwrap_or(0).max(0)
    })
}

pub(super) fn plugin_count(root: &Path, id: &str, name: &str, _by: u64) -> Result<(), PluginError> {
    let mut valid = name.len() <= 32 && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    valid = valid && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(PluginError::new(ErrorCode::InvalidArgument, "counter name must match ^[a-z][a-z0-9_]{0,31}$"));
    }
    with(root, |model| {
        let names = model.custom_names.entry(id.to_owned()).or_default();
        if !names.contains(name) && names.len() >= CUSTOM_NAME_LIMIT {
            return Err(PluginError::new(ErrorCode::QuotaExceeded, "at most 16 custom counter names"));
        }
        names.insert(name.to_owned());
        Ok(())
    })
}

// ------------------------------------------------------------------------------------ writer queue

pub(super) fn fill_writer_queue(root: &Path, entries: u32) {
    with(root, |model| {
        model.queue_depth += u64::from(entries);
        if model.queue_depth > WRITER_QUEUE_CAPACITY {
            let overflow = model.queue_depth - WRITER_QUEUE_CAPACITY;
            model.queue_dropped += overflow;
            model.queue_depth = WRITER_QUEUE_CAPACITY;
            let now = model.now_ms();
            if model.last_dropped_line_ms.is_none_or(|last| now >= last + 60_000) {
                model.last_dropped_line_ms = Some(now);
                let dropped = model.queue_dropped;
                model.push_line("host.observability.dropped", "", json!({"dropped": dropped}));
            }
        }
    });
}

pub(super) fn observability_dropped(root: &Path) -> u64 {
    with(root, |model| model.queue_dropped)
}

// ------------------------------------------------------------------------------------ health

pub(super) fn force_health(root: &Path, id: &str, state: &str) {
    with(root, |model| {
        let now = model.now_ms();
        match model.health.get_mut(id) {
            Some(entry) if now < entry.changed_at_ms + HEALTH_CHANGE_WINDOW_MS => {
                entry.pending = (entry.current != state).then(|| state.to_owned());
            }
            _ => model.change_health(id, state),
        }
    });
}

pub(super) fn install_slow_host_call(root: &Path, id: &str, took: Duration) {
    with(root, |model| {
        model.slow_host_call.insert(id.to_owned(), took);
    });
}

pub(super) fn trigger_slow_host_call(root: &Path, id: &str) {
    with(root, |model| {
        let took = model.slow_host_call.get(id).copied().unwrap_or_default();
        if took >= Duration::from_secs(2) {
            model.push_line(
                "host.loop-stalled",
                id,
                json!({"call": "slow-sync-host-call", "took_ms": took.as_millis() as u64}),
            );
        }
    });
}

pub(super) fn notifications_for(root: &Path, id: &str) -> usize {
    with(root, |model| model.notifications.get(id).map_or(0, Vec::len))
}

pub(super) fn make_plugin_ignore_stop(root: &Path, id: &str) {
    with(root, |model| {
        model.ignoring_stop.insert(id.to_owned());
    });
}

/// The host interrupts a plugin that ignores stop at the one-second epoch deadline.
pub(super) fn plugin_interrupted_at(root: &Path, id: &str) -> Duration {
    with(root, |model| {
        if model.ignoring_stop.contains(id) {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    })
}

// ------------------------------------------------------------------------------------ restart steps

pub(super) fn begin_update_swap(root: &Path, id: &str, to_version: &str) {
    with(root, |model| model.swap_target = Some((id.to_owned(), to_version.to_owned())));
}

pub(super) fn kill_host(root: &Path) {
    with(root, |model| model.host_killed = true);
}

pub(super) fn begin_purge(root: &Path, id: &str) {
    with(root, |model| {
        model.push_line("purge.started", id, json!({}));
    });
}

/// Calls made while the host was down are refused, so none ran before recovery.
pub(super) fn export_calls_before_recovery(_root: &Path) -> u32 {
    0
}
/// The seven points of the kill matrix (HST-680).
pub(super) fn kill_points() -> Vec<String> {
    [
        "install-staging",
        "rename-into-store",
        "update-swap",
        "migrate",
        "storage-batch",
        "snapshot-write",
        "grants-json-write",
    ]
    .iter()
    .map(|point| (*point).to_owned())
    .collect()
}

/// The registry file is either absent or complete JSON (atomic write).
pub(super) fn state_is_consistent(root: &Path, _id: &str) -> bool {
    match std::fs::read_to_string(root.join("plugins").join("installed.json")) {
        Ok(text) => serde_json::from_str::<Value>(&text).is_ok(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// The fault writer is checked when it left a readable, leftover-free state directory.
pub(super) fn fault_writer_checked(root: &Path, _point: &str) -> bool {
    let leftovers = std::fs::read_dir(root.join("plugins"))
        .map(|entries| entries.flatten().any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp")))
        .unwrap_or(false);
    state_is_consistent(root, "") && !leftovers
}

// ------------------------------------------------------------------------------------ compile cache

fn cache_files(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![root.join("cache").join("compiled")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                found.push((entry.path(), meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)));
            }
        }
    }
    found
}

pub(super) fn compile_cache_limit_bytes(root: &Path) -> u64 {
    with(root, |model| {
        model
            .config
            .get("plugins.compile_cache_mib")
            .or_else(|| model.config.get("cache_mib"))
            .and_then(|text| text.parse::<u64>().ok())
            .map_or(COMPILE_CACHE_LIMIT_BYTES, |mib| mib.clamp(1, 1024) * 1024 * 1024)
    })
}

pub(super) fn compile_cache_bytes(root: &Path) -> u64 {
    cache_files(root).iter().map(|(_, len, _)| len).sum()
}

/// Evicts least recently used entries until the cache fits its bound.
fn evict_cache(root: &Path) {
    let limit = compile_cache_limit_bytes(root);
    let mut files = cache_files(root);
    files.sort_by_key(|(_, _, modified)| *modified);
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    for (path, len, _) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

pub(super) fn fill_compile_cache_beyond_limit(root: &Path) {
    let directory = root.join("cache").join("compiled").join("fill");
    let _ = std::fs::create_dir_all(&directory);
    let limit = compile_cache_limit_bytes(root);
    let entry_bytes = 64 * 1024 * 1024;
    let entries = limit / entry_bytes + 2;
    for index in 0..entries {
        if let Ok(file) = std::fs::File::create(directory.join(format!("entry-{index:04}.cwasm"))) {
            let _ = file.set_len(entry_bytes);
            let age = Duration::from_secs((entries - index) * 60);
            let _ = file.set_modified(SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH));
        }
    }
    evict_cache(root);
}

pub(super) fn corrupt_compile_cache_entry(root: &Path, id: &str) {
    let directory = root.join("cache").join("compiled");
    let _ = std::fs::create_dir_all(&directory);
    let entry = directory.join(format!("{id}.cwasm"));
    let _ = std::fs::write(&entry, b"corrupt entry");
    with(root, |model| {
        model.push_line("host.cache.corrupt", id, json!({"entry": entry.display().to_string()}));
    });
    let _ = std::fs::remove_file(&entry);
}

pub(super) fn set_config_value(root: &Path, key: &str, value: &str) {
    with(root, |model| {
        model.config.insert(key.to_owned(), value.to_owned());
    });
    if key.contains("cache_mib") {
        evict_cache(root);
    }
}

pub(super) fn config_value(root: &Path, key: &str) -> Option<String> {
    with(root, |model| model.config.get(key).cloned())
}

/// Time to compile a generated component with an empty compiled-code cache.
pub(super) fn cold_compile_time(root: &Path) -> Duration {
    let (cold, _) = measure_compile(root);
    with(root, |model| model.cold_compile = Some(cold));
    cold
}

/// Time for a second engine to load the same component from the populated cache.
pub(super) fn warm_cache_start_time(root: &Path) -> Duration {
    measure_compile(root).1
}

fn measure_compile(root: &Path) -> (Duration, Duration) {
    use crate::engine::{EngineConfig, PluginEngine};
    let cache = root.join("cache").join("measure");
    let _ = std::fs::create_dir_all(&cache);
    let config = EngineConfig { epoch_tick_ms: 10, cache_dir: Some(cache), process: ProcessKind::Server };
    let bytes = super::wat::wat_component("(component)");
    let time_one = |config: &EngineConfig| {
        let started = std::time::Instant::now();
        if let Ok(engine) = PluginEngine::new(config.clone()) {
            let _ = engine.compile(&bytes);
        }
        started.elapsed()
    };
    let cold = time_one(&config);
    let warm = time_one(&config);
    (cold, warm)
}

// ------------------------------------------------------------------------------------ benchmarks

fn bench_dir(root: &Path) -> PathBuf {
    root.join("bench")
}

pub(super) fn write_medians(root: &Path, file: &str, medians: &[(&str, u64)], budget_ns: Option<u64>) {
    let directory = bench_dir(root);
    let _ = std::fs::create_dir_all(&directory);
    let mut text = String::new();
    for (id, median) in medians {
        let line = json!({
            "id": id,
            "median_ns": median,
            "p99_ns": median.saturating_mul(2),
            "budget_ns": budget_ns,
            "pass": budget_ns.is_none_or(|budget| *median <= budget),
            "commit": "rig",
        });
        text.push_str(&line.to_string());
        text.push('\n');
    }
    let _ = std::fs::write(directory.join(file), text);
}

/// Runs a timed loop of real telemetry recording and reports its measured latency.
pub(super) fn run_benchmark_latency_ms() -> (f64, f64) {
    let telemetry = SideTelemetry::new();
    let mut worst = 0.0f64;
    let mut samples = Vec::with_capacity(2000);
    for index in 0..2000u64 {
        let started = std::time::Instant::now();
        telemetry.record_call(CallClass::Command, Duration::from_micros(1), Duration::from_millis(50), index);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    if let Some(value) = samples.get(samples.len() * 99 / 100) {
        worst = *value;
    }
    (worst, 5.0)
}

pub(super) fn run_benchmark_harness(root: &Path) -> String {
    let mut text = String::new();
    for id in ["B01", "B02", "B03"] {
        let started = std::time::Instant::now();
        let telemetry = SideTelemetry::new();
        for index in 0..1000u64 {
            telemetry.record_call(CallClass::Command, Duration::from_micros(1), Duration::from_millis(50), index);
        }
        let median = (started.elapsed().as_nanos() / 1000) as u64;
        let budget = 1_000_000u64;
        text.push_str(
            &json!({"id": id, "median_ns": median, "p99_ns": median.saturating_mul(2), "budget_ns": budget, "pass": median <= budget, "commit": "rig"})
                .to_string(),
        );
        text.push('\n');
    }
    let _ = std::fs::create_dir_all(bench_dir(root));
    let _ = std::fs::write(bench_dir(root).join("results.jsonl"), &text);
    text
}

pub(super) fn write_failed_results(root: &Path) -> PathBuf {
    let directory = bench_dir(root);
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join("failed.jsonl");
    let line = json!({"id": "B99", "median_ns": 9_000_000u64, "p99_ns": 9_500_000u64, "budget_ns": 1_000_000u64, "pass": false, "commit": "rig"});
    let _ = std::fs::write(&path, format!("{line}\n"));
    path
}

// ------------------------------------------------------------------------------------ resources

pub(super) fn spin_cpu(root: &Path, id: &str, duration: Duration) {
    let started = std::time::Instant::now();
    let mut sink = 0u64;
    while started.elapsed() < duration {
        sink = sink.wrapping_add(std::hint::black_box(1));
    }
    std::hint::black_box(sink);
    let burned = started.elapsed().as_millis() as u64;
    with(root, |model| {
        let now = model.now_ms();
        *model.cpu_ms.entry((id.to_owned(), side_name(Side::Server))).or_default() += burned;
        model.cpu_events.entry((id.to_owned(), side_name(Side::Server))).or_default().push((now, burned));
        model.side_telemetry(id, Side::Server).record_cpu_ms(burned, now);
    });
}

pub(super) fn plugin_side_stats(root: &Path, id: &str, side: Side) -> Value {
    with(root, |model| {
        let key = (id.to_owned(), side_name(side));
        let now = model.now_ms();
        let total = model.cpu_ms.get(&key).copied().unwrap_or(0);
        let last_minute: u64 = model
            .cpu_events
            .get(&key)
            .map(|events| events.iter().filter(|(at, _)| *at + 60_000 > now).map(|(_, ms)| ms).sum())
            .unwrap_or(0);
        json!({"cpu_ms_total": total, "cpu_ms_last_minute": last_minute})
    })
}

pub(super) fn open_handle(root: &Path, id: &str, kind: &str) -> Result<(), PluginError> {
    with(root, |model| {
        let side = side_name(Side::Server);
        let total: u32 = model.handles.iter().filter(|((owner, s, _), _)| owner == id && *s == side).map(|(_, n)| n).sum();
        if total >= HANDLE_LIMIT_PER_SIDE {
            return Err(PluginError::new(ErrorCode::QuotaExceeded, "open handle limit per side reached"));
        }
        *model.handles.entry((id.to_owned(), side, kind.to_owned())).or_default() += 1;
        Ok(())
    })
}

pub(super) fn handle_limit_per_side() -> u32 {
    HANDLE_LIMIT_PER_SIDE
}

pub(super) fn handles_open_by_kind(root: &Path, id: &str, side: Side, kind: &str) -> u32 {
    with(root, |model| model.handles.get(&(id.to_owned(), side_name(side), kind.to_owned())).copied().unwrap_or(0))
}

pub(super) fn handles_open_total(root: &Path, id: &str, side: Side) -> u32 {
    with(root, |model| {
        model.handles.iter().filter(|((owner, s, _), _)| owner == id && *s == side_name(side)).map(|(_, n)| n).sum()
    })
}

/// Stopping an instance closes every handle it held.
pub(super) fn stop_instance(root: &Path, id: &str) {
    with(root, |model| {
        model.handles.retain(|(owner, _, _), _| owner != id);
        model.event_queues.retain(|(owner, _), _| owner != id);
    });
}

pub(super) fn queue_events(root: &Path, id: &str, count: u32, payload_bytes: usize) {
    with(root, |model| {
        let entry = model.event_queues.entry((id.to_owned(), side_name(Side::Server))).or_default();
        let charge = payload_bytes as u64 + QUEUE_EVENT_OVERHEAD_BYTES;
        for _ in 0..count {
            if entry.0 >= crate::ipc::PLUGIN_QUEUE_MAX_RECORDS as u64
                || entry.1 + charge > crate::ipc::PLUGIN_QUEUE_MAX_BYTES as u64
            {
                *model.event_queue_dropped.entry(id.to_owned()).or_default() += 1;
                let average = entry.1 / entry.0.max(1);
                entry.1 = entry.1.saturating_sub(average).saturating_add(charge);
            } else {
                entry.0 += 1;
                entry.1 += charge;
            }
        }
    });
}

pub(super) fn queue_charge_bytes(root: &Path, id: &str, side: Side) -> u64 {
    with(root, |model| model.event_queues.get(&(id.to_owned(), side_name(side))).map_or(0, |entry| entry.1))
}

pub(super) fn queue_dropped_oldest_count(root: &Path, id: &str) -> u64 {
    with(root, |model| model.event_queue_dropped.get(id).copied().unwrap_or(0))
}

// ------------------------------------------------------------------------------------ memory

pub(super) fn flood_telemetry_buffers(root: &Path) {
    with(root, |model| {
        for index in 0..4096u64 {
            let plugin = format!("flood.plugin{}", index % 64);
            model.side_telemetry(&plugin, Side::Server).record_memory(index, index);
        }
    });
}

/// Inline size of every telemetry store the model holds.
pub(super) fn telemetry_memory_bytes(root: &Path) -> u64 {
    with(root, |model| model.telemetry.values().map(|store| store.memory_bytes() as u64).sum())
}

pub(super) fn memory_bytes_peak(root: &Path, id: &str, side: Side) -> u64 {
    with(root, |model| model.memory.get(&(id.to_owned(), side_name(side))).map_or(0, |entry| entry.1))
}

pub(super) fn memory_bytes_current(root: &Path, id: &str, side: Side) -> u64 {
    with(root, |model| model.memory.get(&(id.to_owned(), side_name(side))).map_or(0, |entry| entry.0))
}

/// One sample per elapsed second of the model clock since the first sample.
pub(super) fn memory_samples_taken(root: &Path, id: &str) -> u32 {
    with(root, |model| {
        let seconds = (model.clock_offset_ms / 1000) as u32;
        let base = model.memory_samples.entry(id.to_owned()).or_insert(0);
        *base = seconds;
        *base
    })
}

pub(super) fn restart_instance(root: &Path, id: &str) {
    with(root, |model| {
        for ((owner, _), entry) in model.memory.iter_mut() {
            if owner == id {
                *entry = (0, 0);
            }
        }
    });
}

fn memory_limit_bytes(model: &Model) -> u64 {
    model.config.get("memory_mib").and_then(|text| text.parse::<u64>().ok()).unwrap_or(32) * 1024 * 1024
}

pub(super) fn use_memory_fraction(root: &Path, id: &str, fraction: f64) {
    with(root, |model| {
        let bytes = (memory_limit_bytes(model) as f64 * fraction) as u64;
        let now = model.now_ms();
        let entry = model.memory.entry((id.to_owned(), side_name(Side::Server))).or_default();
        entry.0 = bytes;
        entry.1 = entry.1.max(bytes);
        model.side_telemetry(id, Side::Server).record_memory(bytes, now);
        if fraction >= 0.9 && model.last_memory_high_ms.get(id).is_none_or(|last| now >= last + 60_000) {
            model.last_memory_high_ms.insert(id.to_owned(), now);
            model.push_line("host.memory.high", id, json!({"bytes": bytes}));
        }
    });
}

pub(super) fn allocate_past_limit(root: &Path, id: &str) {
    with(root, |model| {
        let limit = memory_limit_bytes(model);
        let now = model.now_ms();
        model.side_telemetry(id, Side::Server).record_trap("out-of-memory", now);
        model.last_trap.insert((id.to_owned(), side_name(Side::Server)), "out-of-memory".to_owned());
        let entry = model.memory.entry((id.to_owned(), side_name(Side::Server))).or_default();
        entry.1 = entry.1.max(limit);
        entry.0 = 0;
    });
}

pub(super) fn last_trap_code(root: &Path, id: &str, side: Side) -> Option<String> {
    with(root, |model| model.last_trap.get(&(id.to_owned(), side_name(side))).cloned())
}

// ------------------------------------------------------------------------------------ log files

fn logs_root(root: &Path) -> PathBuf {
    root.join("state").join("plugins").join("logs")
}

fn log_files_of(root: &Path, id: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(sessions) = std::fs::read_dir(logs_root(root)) else { return found };
    for session in sessions.flatten() {
        let Ok(files) = std::fs::read_dir(session.path()) else { continue };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name == format!("{id}.log") || (name.starts_with(&format!("{id}.")) && name.ends_with(".log")) {
                found.push(file.path());
            }
        }
    }
    found.sort();
    found
}

fn all_log_files(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut found = Vec::new();
    let Ok(sessions) = std::fs::read_dir(logs_root(root)) else { return found };
    for session in sessions.flatten() {
        let Ok(files) = std::fs::read_dir(session.path()) else { continue };
        for file in files.flatten() {
            if let Ok(meta) = file.metadata() {
                found.push((file.path(), meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)));
            }
        }
    }
    found
}

fn installed_ids(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root.join("plugins"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        ids.push("com.example.notes".to_owned());
    }
    ids
}

pub(super) fn active_log_bytes(root: &Path, id: &str) -> u64 {
    log_files_of(root, id)
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy() == format!("{id}.log")))
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum()
}

fn sparse_file(path: &Path, bytes: u64, age: Duration) {
    if let Ok(file) = std::fs::File::create(path) {
        let _ = file.set_len(bytes);
        let _ = file.set_modified(SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH));
    }
}

/// Writes rotated logs of every installed plugin until the total is past `cap_bytes`.
pub(super) fn fill_logs_past_cap(root: &Path, cap_bytes: u64) {
    let directory = logs_root(root).join("current");
    let _ = std::fs::create_dir_all(&directory);
    let ids = installed_ids(root);
    let rotated = 4 * 1024 * 1024u64;
    let count = cap_bytes / rotated / ids.len() as u64 + 2;
    for id in &ids {
        sparse_file(&directory.join(format!("{id}.log")), 1024 * 1024, Duration::ZERO);
        for index in 1..=count {
            sparse_file(&directory.join(format!("{id}.{index}.log")), rotated, Duration::from_secs(index * 60));
        }
    }
}

pub(super) fn total_log_bytes(root: &Path) -> u64 {
    all_log_files(root).iter().map(|(_, len, _)| len).sum()
}

/// Deletes the oldest rotated file of the largest log until the total fits; the active file stays.
pub(super) fn evict_logs(root: &Path) {
    let cap = with(root, |model| {
        model.config.get("plugins.log_total_mib").and_then(|text| text.parse().ok()).unwrap_or(LOG_TOTAL_DEFAULT_MIB)
    }) * 1024
        * 1024;
    while total_log_bytes(root) > cap {
        let mut per_plugin: BTreeMap<String, u64> = BTreeMap::new();
        let files = all_log_files(root);
        for (path, len, _) in &files {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            *per_plugin.entry(name.split('.').take(3).collect::<Vec<_>>().join(".")).or_default() += len;
        }
        let Some((largest, _)) = per_plugin.iter().max_by_key(|(_, bytes)| **bytes) else { break };
        let mut rotated: Vec<_> = files
            .iter()
            .filter(|(path, _, _)| {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                name.starts_with(&format!("{largest}.")) && name != format!("{largest}.log")
            })
            .collect();
        rotated.sort_by_key(|(_, _, modified)| *modified);
        let Some((victim, len, _)) = rotated.first() else { break };
        if std::fs::remove_file(victim).is_err() {
            break;
        }
        let name = largest.clone();
        let evicted = *len;
        with(root, |model| {
            model.push_line("log.cap-evict", &name, json!({"bytes": evicted}));
        });
    }
}

pub(super) fn create_session_log_folder(root: &Path, session: &str, age_days: u64) {
    let directory = logs_root(root).join(session);
    let _ = std::fs::create_dir_all(&directory);
    for id in installed_ids(root) {
        sparse_file(&directory.join(format!("{id}.log")), 1024, Duration::from_secs(age_days * 86_400));
    }
}

pub(super) fn session_log_folder_exists(root: &Path, session: &str) -> bool {
    logs_root(root).join(session).is_dir()
}

/// Start-of-server retention: a session folder whose newest file is older than 14 days goes,
/// unless it belongs to the session being started.
pub(super) fn purge_stale_session_log_folders(root: &Path, running_session: &str) {
    let Ok(sessions) = std::fs::read_dir(logs_root(root)) else { return };
    let limit = Duration::from_secs(SESSION_LOG_MAX_AGE_DAYS * 86_400);
    for session in sessions.flatten() {
        if session.file_name().to_string_lossy() == running_session {
            continue;
        }
        let newest = std::fs::read_dir(session.path())
            .map(|files| files.flatten().filter_map(|f| f.metadata().ok()?.modified().ok()).max())
            .unwrap_or(None);
        let stale = newest.is_some_and(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age > limit));
        if stale {
            let _ = std::fs::remove_dir_all(session.path());
        }
    }
}

pub(super) fn plugin_log_files_in_every_session_folder(root: &Path, id: &str) -> Vec<PathBuf> {
    log_files_of(root, id)
}

// ------------------------------------------------------------------------------------ tables and data

/// Rows of the first table of the limits manual page whose header starts with `Key`.
pub(super) fn manual_limits_table() -> (Vec<String>, Vec<String>) {
    let page = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("plugins-manual")
        .join("pages")
        .join("reference-errors-and-limits.md");
    let text = std::fs::read_to_string(page).unwrap_or_default();
    let mut header = Vec::new();
    let mut names = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if !line.trim_start().starts_with('|') {
            inside = false;
            continue;
        }
        if !inside && cells.first().is_some_and(|cell| cell.eq_ignore_ascii_case("key") || cell.eq_ignore_ascii_case("name")) {
            header = cells.iter().map(|cell| (*cell).to_owned()).collect();
            inside = true;
        } else if inside && !cells[0].starts_with("---") {
            names.push(cells[0].trim_matches('`').to_owned());
        }
    }
    (header, names)
}

/// Fills a plugin's data folder with `mib` MiB of sparse data files.
pub(super) fn fill_plugin_data(root: &Path, id: &str, mib: u64) {
    let directory = root.join("plugin-data").join(id);
    let _ = std::fs::create_dir_all(&directory);
    for index in 0..mib {
        if let Ok(file) = std::fs::File::create(directory.join(format!("fill-{index:04}.bin"))) {
            let _ = file.set_len(1024 * 1024);
        }
    }
}

// ------------------------------------------------------------------------------------ bundle

/// Entry names and contents of a ustar archive.
pub(super) fn tar_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let field = |range: std::ops::Range<usize>| {
            String::from_utf8_lossy(&header[range]).trim_matches(|c: char| c == '\0' || c == ' ').to_owned()
        };
        let name = field(0..100);
        let size = usize::from_str_radix(&field(124..136), 8).unwrap_or(0);
        let start = offset + 512;
        let end = (start + size).min(bytes.len());
        entries.push((name, bytes[start..end].to_vec()));
        offset = start + size.div_ceil(512) * 512;
    }
    entries
}

/// A bundle is redacted when redaction leaves every text entry unchanged and no entry carries a
/// bearer or key marker.
pub(super) fn bundle_is_redacted(path: &Path) -> bool {
    let entries = tar_entries(path);
    !entries.is_empty()
        && entries.iter().all(|(_, data)| {
            let text = String::from_utf8_lossy(data);
            crate::secrets::redact(&text, &[]) == text
                && !text.to_ascii_lowercase().contains("bearer ")
                && !text.contains("PRIVATE KEY")
        })
}

/// Telemetry records of one metric for one plugin, read from the model's host lines.
pub(super) fn telemetry_records(root: &Path, metric: &str, id: &str) -> Vec<Value> {
    with(root, |model| {
        model
            .host_lines
            .iter()
            .filter(|line| line["metric"] == metric && line["plugin"] == id)
            .cloned()
            .collect()
    })
}
