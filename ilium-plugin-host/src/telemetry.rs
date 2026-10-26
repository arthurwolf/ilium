//! Counters, traces and the log ring buffer behind `ilium plugin trace/profile/logs`
//! (reference-cli.md).

use std::collections::{BTreeMap, VecDeque};
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
pub struct LogRing {
    capacity: usize,
    lines: VecDeque<LogLine>,
}

impl LogRing {
    /// A ring that keeps at most `capacity` lines.
    pub fn new(capacity: usize) -> LogRing {
        Self {
            capacity,
            lines: VecDeque::new(),
        }
    }

    /// Adds a line, dropping the oldest at capacity.
    pub fn push(&mut self, mut line: LogLine) {
        if self.capacity == 0 {
            return;
        }
        line.message = truncate_message(&line.message);
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// The lines of a plugin, oldest first.
    pub fn lines_of(&self, plugin: &str) -> Vec<LogLine> {
        self.lines
            .iter()
            .filter(|line| line.plugin == plugin)
            .cloned()
            .collect()
    }
}

/// Cuts one WASI stderr line to the log line limit (reference-host-runtime.md HST-078).
pub fn cut_wasi_line(line: &str) -> String {
    cut_message(line, LOG_MESSAGE_MAX_BYTES, "[cut]")
}
fn line_safe(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn cut_message(message: &str, limit: usize, marker: &str) -> String {
    if message.len() <= limit {
        return message.into();
    }
    let mut end = limit.saturating_sub(marker.len());
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{marker}", &message[..end])
}

// ---------------------------------------------------------------------------------------------
// Near-miss tracking (reference-host-budgets.md HST-313).
// ---------------------------------------------------------------------------------------------

/// Counts calls that used 80 percent or more of their deadline within a five-minute window
/// (reference-host-budgets.md HST-313).
#[derive(Debug, Default)]
pub struct NearMissTracker {
    times: VecDeque<(u64, usize)>,
    last_ms: u64,
}

impl NearMissTracker {
    /// An empty tracker.
    pub fn new() -> NearMissTracker {
        Self::default()
    }

    /// Records one call; returns whether it was a near miss (HST-313).
    pub fn record_call(&mut self, now_ms: u64, deadline_ms: u64, took_ms: u64) -> bool {
        // Inputs are elapsed monotonic milliseconds. Late calls are breaches, not near misses.
        self.last_ms = self.last_ms.max(now_ms);
        while self
            .times
            .front()
            .is_some_and(|(at, _)| self.last_ms.saturating_sub(*at) >= 300_000)
        {
            self.times.pop_front();
        }
        let near = deadline_ms != 0
            && took_ms <= deadline_ms
            && u128::from(took_ms) * 5 >= u128::from(deadline_ms) * 4;
        if near && now_ms == self.last_ms {
            if let Some((_, count)) = self.times.back_mut().filter(|(at, _)| *at == now_ms) {
                *count = count.saturating_add(1);
            } else {
                self.times.push_back((now_ms, 1));
            }
        }
        near
    }

    /// Near misses inside the window ending at `now_ms` (HST-313).
    pub fn count_in_window(&self, now_ms: u64) -> usize {
        self.times
            .iter()
            .filter(|(at, _)| *at <= now_ms && now_ms.saturating_sub(*at) < 300_000)
            .fold(0_usize, |total, (_, count)| total.saturating_add(*count))
    }

    /// Whether the plugin counts as degraded at `now_ms` (HST-313).
    pub fn is_degraded(&self, now_ms: u64) -> bool {
        self.count_in_window(now_ms) >= 5
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
    /// Start of the hour in the host process-monotonic clock, in milliseconds.
    pub hour_start_ms: u64,
    counters: std::collections::BTreeMap<String, u64>,
}

const TELEMETRY_CLASSES: [CallClass; 15] = [
    CallClass::Event,
    CallClass::Command,
    CallClass::Render,
    CallClass::Annotate,
    CallClass::Frame,
    CallClass::Detector,
    CallClass::Activate,
    CallClass::Deactivate,
    CallClass::Migrate,
    CallClass::Job,
    CallClass::MenuItems,
    CallClass::QueryCatalog,
    CallClass::ListVirtualChildren,
    CallClass::Tokens,
    CallClass::SettingsValidate,
];

fn class_name(class: CallClass) -> &'static str {
    match class {
        CallClass::Event => "event",
        CallClass::Command => "command",
        CallClass::Render => "render",
        CallClass::Annotate => "annotate",
        CallClass::Frame => "frame",
        CallClass::Detector => "detector",
        CallClass::Activate => "activate",
        CallClass::Deactivate => "deactivate",
        CallClass::Migrate => "migrate",
        CallClass::Job => "job",
        CallClass::MenuItems => "menu-items",
        CallClass::QueryCatalog => "query-catalog",
        CallClass::ListVirtualChildren => "list-virtual-children",
        CallClass::Tokens => "tokens",
        CallClass::SettingsValidate => "settings-validate",
    }
}

impl HourBucket {
    fn value(&self, family: &str, key: &str) -> u64 {
        self.counters
            .get(&format!("{family}:{key}"))
            .copied()
            .unwrap_or(0)
    }

    fn histogram(&self, family: &str, key: &str) -> [u64; 13] {
        std::array::from_fn(|index| self.value(family, &format!("{key}:{index}")))
    }

    /// Calls of one class.
    pub fn calls(&self, class: CallClass) -> u64 {
        self.value("calls", class_name(class))
    }
    /// The 13-bucket duration histogram of one class.
    pub fn duration_histogram(&self, class: CallClass) -> [u64; 13] {
        self.histogram("duration", class_name(class))
    }
    /// Near misses of one class.
    pub fn near_misses(&self, class: CallClass) -> u64 {
        self.value("near_misses", class_name(class))
    }
    /// Deadline breaches of one class and kind.
    pub fn breaches(&self, class: CallClass, kind: BreachKind) -> u64 {
        let kind = match kind {
            BreachKind::Hard => "hard",
            BreachKind::Soft => "soft",
        };
        self.value("breaches", &format!("{}:{kind}", class_name(class)))
    }
    /// Traps by code.
    pub fn traps(&self, code: &str) -> u64 {
        self.value("traps", code)
    }
    /// The queue-depth high-water mark.
    pub fn queue_depth_high_water(&self) -> u64 {
        self.value("queue_depth", "")
    }
    /// Events dropped by topic.
    pub fn events_dropped(&self, topic: &str) -> u64 {
        self.value("events_dropped", topic)
    }
    /// Events coalesced by topic.
    pub fn events_coalesced(&self, topic: &str) -> u64 {
        self.value("events_coalesced", topic)
    }
    /// Quota rejections.
    pub fn quota_rejections(&self) -> u64 {
        self.value("quota_rejections", "")
    }
    /// Rate-limited calls by import name.
    pub fn rate_limited(&self, import: &str) -> u64 {
        self.value("rate_limited", import)
    }
    /// CPU milliseconds.
    pub fn cpu_ms(&self) -> u64 {
        self.value("cpu_ms", "")
    }
    /// Peak memory in bytes.
    pub fn memory_peak(&self) -> u64 {
        self.value("memory_peak", "")
    }
    /// Restarts by reason.
    pub fn restarts(&self, reason: &str) -> u64 {
        self.value("restarts", reason)
    }
    /// Failures by reason.
    pub fn failures(&self, reason: &str) -> u64 {
        self.value("failures", reason)
    }
    /// The helper round-trip histogram.
    pub fn helper_round_trips(&self) -> [u64; 13] {
        self.histogram("helper_round_trips", "")
    }
    /// The render-time histogram of one view.
    pub fn render_ms(&self, view: &str) -> [u64; 13] {
        self.histogram("render_ms", view)
    }
}

const HOUR_MS: u64 = 3_600_000;
const RETAINED_HOURS: usize = 25;
const CLASS_COUNTERS: usize = 17;
const FIXED_COUNTERS: usize = 15 * CLASS_COUNTERS + 17;
const NAME_SLOTS: usize = 12;
const NAME_BYTES: usize = 48;
const NAMED_COUNTERS: usize = NAME_SLOTS + 6 + 26;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// Process-monotonic milliseconds from one unchanged host clock origin.
/// Wall-clock timestamps must not be supplied: clock adjustments cannot move retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryTime(pub u64);

/// Whether an observation was retained. Rejected observations remain the caller's responsibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterAdmission {
    /// Exact metric and name retained.
    Recorded,
    /// Numeric observation retained in the family's `__overflow__` series.
    Aggregated,
    /// Host must prepare this hour with exclusive clock-maintenance custody, then retry.
    ClockNotPrepared,
    /// Observation belongs to an already expired hour and was not retained.
    Expired,
}

#[derive(Debug)]
struct CounterHour {
    hour: u64,
    touched: AtomicBool,
    active: AtomicUsize,
    completed: AtomicU64,
    fixed: [AtomicU64; FIXED_COUNTERS],
    named: [AtomicU64; NAMED_COUNTERS],
}

impl CounterHour {
    fn new(hour: u64) -> Self {
        Self {
            hour,
            touched: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            completed: AtomicU64::new(0),
            fixed: std::array::from_fn(|_| AtomicU64::new(0)),
            named: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn update(&self, operation: impl FnOnce(&Self)) {
        self.active.fetch_add(1, Ordering::SeqCst);
        operation(self);
        self.touched.store(true, Ordering::Release);
        self.completed.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct CounterName {
    hash: AtomicU64,
    ready: AtomicBool,
    family: AtomicU8,
    bytes: [AtomicU8; NAME_BYTES],
}

impl CounterName {
    fn new() -> Self {
        Self {
            hash: AtomicU64::new(0),
            ready: AtomicBool::new(false),
            family: AtomicU8::new(0),
            bytes: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }

    fn name(&self) -> Option<(usize, String)> {
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        let bytes: Vec<u8> = self
            .bytes
            .iter()
            .map(|byte| byte.load(Ordering::Relaxed))
            .take_while(|byte| *byte != 0)
            .collect();
        Some((
            usize::from(self.family.load(Ordering::Relaxed)),
            String::from_utf8(bytes).ok()?,
        ))
    }

    fn matches(&self, family: usize, name: &str) -> bool {
        self.ready.load(Ordering::Acquire)
            && usize::from(self.family.load(Ordering::Relaxed)) == family
            && self
                .bytes
                .iter()
                .map(|byte| byte.load(Ordering::Relaxed))
                .take_while(|byte| *byte != 0)
                .eq(name.bytes())
    }

    fn claim(&self, hash: u64, family: usize, name: &str) -> bool {
        if self
            .hash
            .compare_exchange(0, hash, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.family.store(family as u8, Ordering::Relaxed);
        for (destination, byte) in self.bytes.iter().zip(name.bytes()) {
            destination.store(byte, Ordering::Relaxed);
        }
        self.ready.store(true, Ordering::Release);
        true
    }
}

fn add_counter(counter: &AtomicU64, count: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(count))
    });
}

fn duration_bucket(took: Duration) -> usize {
    const NANOS: [u128; 12] = [
        100_000,
        200_000,
        500_000,
        1_000_000,
        2_000_000,
        5_000_000,
        10_000_000,
        20_000_000,
        50_000_000,
        100_000_000,
        500_000_000,
        1_000_000_000,
    ];
    NANOS
        .iter()
        .position(|bound| took.as_nanos() <= *bound)
        .unwrap_or(12)
}

fn class_offset(class: CallClass) -> usize {
    let index = match class {
        CallClass::Event => 0,
        CallClass::Command => 1,
        CallClass::Render => 2,
        CallClass::Annotate => 3,
        CallClass::Frame => 4,
        CallClass::Detector => 5,
        CallClass::Activate => 6,
        CallClass::Deactivate => 7,
        CallClass::Migrate => 8,
        CallClass::Job => 9,
        CallClass::MenuItems => 10,
        CallClass::QueryCatalog => 11,
        CallClass::ListVirtualChildren => 12,
        CallClass::Tokens => 13,
        CallClass::SettingsValidate => 14,
    };
    index * CLASS_COUNTERS
}

const NAMED_FAMILIES: [&str; 6] = [
    "traps",
    "events_dropped",
    "events_coalesced",
    "rate_limited",
    "restarts",
    "failures",
];

fn metric_name_hash(family: usize, name: &str) -> Option<u64> {
    // Host-owned identifiers only. Paths, URLs and arbitrary text are not metric names.
    if name.is_empty()
        || name == "__overflow__"
        || name.len() > NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let mut hash = 0xcbf29ce484222325u64 ^ family as u64;
    for byte in name.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    Some(hash.max(1))
}

/// Fixed-storage counters of one plugin side. Recording uses only atomics and never allocates.
///
/// The host supplies a process-monotonic clock and calls `advance_clock` under exclusive
/// custody before admitting observations outside the prepared window. This explicit maintenance
/// seam prevents resetting counters underneath a delayed concurrent writer. Callers must handle
/// `ClockNotPrepared` by retaining the observation, preparing its hour, and retrying it.
/// `new` is the zero-origin convenience; production should use `with_clock`.
#[derive(Debug)]
pub struct SideTelemetry {
    hours: [CounterHour; RETAINED_HOURS],
    names: [CounterName; NAME_SLOTS],
    render_name: CounterName,
    newest_prepared: u64,
    ended: AtomicBool,
    lifetime_calls: AtomicU64,
    clock_refusals: AtomicU64,
    expired_refusals: AtomicU64,
}

// Capacity is a compile-time invariant, including all names and hour metadata.
const _: () = assert!(std::mem::size_of::<SideTelemetry>() <= TELEMETRY_MAX_BYTES_PER_SIDE);

impl Default for SideTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl SideTelemetry {
    /// Empty counters with a zero process-clock origin.
    pub fn new() -> Self {
        Self::with_clock(TelemetryTime(0))
    }

    /// Empty counters prepared for the current and following 24 process-monotonic hours.
    pub fn with_clock(now: TelemetryTime) -> Self {
        let first = now.0 / HOUR_MS;
        let newest_prepared = first.saturating_add((RETAINED_HOURS - 1) as u64);
        Self {
            hours: std::array::from_fn(|index| {
                let offset = (index + RETAINED_HOURS - (first % RETAINED_HOURS as u64) as usize)
                    % RETAINED_HOURS;
                CounterHour::new(first.saturating_add(offset as u64))
            }),
            names: std::array::from_fn(|_| CounterName::new()),
            render_name: CounterName::new(),
            newest_prepared,
            ended: AtomicBool::new(false),
            lifetime_calls: AtomicU64::new(0),
            clock_refusals: AtomicU64::new(0),
            expired_refusals: AtomicU64::new(0),
        }
    }

    /// Prepares an hour before recording. Exclusive ownership makes recycling race-free.
    /// Monotonic clock regressions never reset a newer bucket. Old observations are refused.
    pub fn advance_clock(&mut self, now: TelemetryTime) {
        let hour = now.0 / HOUR_MS;
        if hour <= self.newest_prepared {
            return;
        }
        let first = hour.saturating_sub((RETAINED_HOURS - 1) as u64);
        for entry in &mut self.hours {
            if entry.hour < first || entry.hour > hour {
                let index = entry.hour % RETAINED_HOURS as u64;
                let offset = (index + RETAINED_HOURS as u64 - first % RETAINED_HOURS as u64)
                    % RETAINED_HOURS as u64;
                *entry = CounterHour::new(first.saturating_add(offset));
            }
        }
        self.newest_prepared = hour;
    }

    fn hour(&self, now_ms: u64) -> Result<&CounterHour, CounterAdmission> {
        let hour = now_ms / HOUR_MS;
        let entry = &self.hours[(hour % RETAINED_HOURS as u64) as usize];
        if entry.hour == hour {
            return Ok(entry);
        }
        if hour > self.newest_prepared {
            add_counter(&self.clock_refusals, 1);
            Err(CounterAdmission::ClockNotPrepared)
        } else {
            add_counter(&self.expired_refusals, 1);
            Err(CounterAdmission::Expired)
        }
    }

    /// Records one call using actual elapsed duration and its resolved deadline.
    pub fn record_call(
        &self,
        class: CallClass,
        took: Duration,
        deadline: Duration,
        now_ms: u64,
    ) -> CounterAdmission {
        let hour = match self.hour(now_ms) {
            Ok(hour) => hour,
            Err(reason) => return reason,
        };
        let offset = class_offset(class);
        let near = !deadline.is_zero()
            && took <= deadline
            && took.as_nanos() * 5 >= deadline.as_nanos() * 4;
        hour.update(|hour| {
            add_counter(&hour.fixed[offset], 1);
            add_counter(&hour.fixed[offset + 1 + duration_bucket(took)], 1);
            if near {
                add_counter(&hour.fixed[offset + 14], 1);
            }
        });
        add_counter(&self.lifetime_calls, 1);
        CounterAdmission::Recorded
    }

    /// Records a hard or soft breach; late calls are not inferred to have a particular kind.
    pub fn record_breach(
        &self,
        class: CallClass,
        kind: BreachKind,
        now_ms: u64,
    ) -> CounterAdmission {
        self.fixed_observation(
            now_ms,
            class_offset(class)
                + match kind {
                    BreachKind::Hard => 15,
                    BreachKind::Soft => 16,
                },
            1,
            false,
        )
    }

    fn fixed_observation(
        &self,
        now_ms: u64,
        index: usize,
        count: u64,
        high_water: bool,
    ) -> CounterAdmission {
        let hour = match self.hour(now_ms) {
            Ok(hour) => hour,
            Err(reason) => return reason,
        };
        hour.update(|hour| {
            if high_water {
                hour.fixed[index].fetch_max(count, Ordering::Relaxed);
            } else {
                add_counter(&hour.fixed[index], count);
            }
        });
        CounterAdmission::Recorded
    }

    fn named_observation(
        &self,
        family: usize,
        name: &str,
        count: u64,
        now_ms: u64,
    ) -> CounterAdmission {
        let hour = match self.hour(now_ms) {
            Ok(hour) => hour,
            Err(reason) => return reason,
        };
        let index = metric_name_hash(family, name).and_then(|hash| {
            for (index, slot) in self.names.iter().enumerate() {
                if slot.matches(family, name) || slot.claim(hash, family, name) {
                    return Some(index);
                }
                // A competing registration of this exact hash is aggregated, never duplicated.
                if slot.hash.load(Ordering::Acquire) == hash && !slot.ready.load(Ordering::Acquire)
                {
                    return None;
                }
            }
            None
        });
        let counter = index.unwrap_or(NAME_SLOTS + family);
        hour.update(|hour| add_counter(&hour.named[counter], count));
        if index.is_some() {
            CounterAdmission::Recorded
        } else {
            CounterAdmission::Aggregated
        }
    }

    /// Records a trap; exhausted or invalid names use the visible overflow series.
    pub fn record_trap(&self, code: &str, now_ms: u64) -> CounterAdmission {
        self.named_observation(0, code, 1, now_ms)
    }
    /// Records queue depth high water.
    pub fn record_queue_depth(&self, depth: u64, now_ms: u64) -> CounterAdmission {
        self.fixed_observation(now_ms, 255, depth, true)
    }
    /// Records actual dropped events, saturating instead of wrapping.
    pub fn record_event_dropped(&self, topic: &str, count: u64, now_ms: u64) -> CounterAdmission {
        self.named_observation(1, topic, count, now_ms)
    }
    /// Records actual coalesced events.
    pub fn record_event_coalesced(&self, topic: &str, count: u64, now_ms: u64) -> CounterAdmission {
        self.named_observation(2, topic, count, now_ms)
    }
    /// Records one quota refusal.
    pub fn record_quota_rejection(&self, now_ms: u64) -> CounterAdmission {
        self.fixed_observation(now_ms, 256, 1, false)
    }
    /// Records one rate-limited import.
    pub fn record_rate_limited(&self, import: &str, now_ms: u64) -> CounterAdmission {
        self.named_observation(3, import, 1, now_ms)
    }
    /// Adds actual thread CPU milliseconds supplied by the host.
    pub fn record_cpu_ms(&self, cpu_ms: u64, now_ms: u64) -> CounterAdmission {
        self.fixed_observation(now_ms, 257, cpu_ms, false)
    }
    /// Records the actual linear-memory-plus-table high water.
    pub fn record_memory(&self, bytes: u64, now_ms: u64) -> CounterAdmission {
        self.fixed_observation(now_ms, 258, bytes, true)
    }
    /// Records a helper round trip in the standard histogram.
    pub fn record_helper_round_trip(&self, took: Duration, now_ms: u64) -> CounterAdmission {
        self.fixed_observation(now_ms, 259 + duration_bucket(took), 1, false)
    }
    /// Records render duration. One named view and one visible overflow histogram are fixed in memory.
    pub fn record_render(&self, view: &str, took: Duration, now_ms: u64) -> CounterAdmission {
        let hour = match self.hour(now_ms) {
            Ok(hour) => hour,
            Err(reason) => return reason,
        };
        let retained = metric_name_hash(6, view).is_some_and(|hash| {
            self.render_name.matches(6, view) || self.render_name.claim(hash, 6, view)
        });
        let index = NAME_SLOTS + 6 + usize::from(!retained) * 13 + duration_bucket(took);
        hour.update(|hour| add_counter(&hour.named[index], 1));
        if retained {
            CounterAdmission::Recorded
        } else {
            CounterAdmission::Aggregated
        }
    }
    /// Records an actual restart.
    pub fn record_restart(&self, reason: &str, now_ms: u64) -> CounterAdmission {
        self.named_observation(4, reason, 1, now_ms)
    }
    /// Records an actual counted failure.
    pub fn record_failure(&self, reason: &str, now_ms: u64) -> CounterAdmission {
        self.named_observation(5, reason, 1, now_ms)
    }

    /// Detached coherent hour snapshots, oldest first. Snapshot allocations belong to the reader.
    /// The partially overlapping oldest hour is retained, yielding at most 25 buckets.
    pub fn snapshot(&self, now_ms: u64) -> Vec<HourBucket> {
        let current = now_ms / HOUR_MS;
        let first = current.saturating_sub(24);
        let mut result = Vec::new();
        for hour in &self.hours {
            if hour.hour < first || hour.hour > current || !hour.touched.load(Ordering::Acquire) {
                continue;
            }
            let (fixed, named) = loop {
                let completed = hour.completed.load(Ordering::SeqCst);
                if hour.active.load(Ordering::SeqCst) != 0 {
                    std::hint::spin_loop();
                    continue;
                }
                let fixed: [u64; FIXED_COUNTERS] =
                    std::array::from_fn(|index| hour.fixed[index].load(Ordering::SeqCst));
                let named: [u64; NAMED_COUNTERS] =
                    std::array::from_fn(|index| hour.named[index].load(Ordering::SeqCst));
                if hour.active.load(Ordering::SeqCst) == 0
                    && completed == hour.completed.load(Ordering::SeqCst)
                {
                    break (fixed, named);
                }
            };
            let mut counters = BTreeMap::new();
            let mut insert = |family: &str, key: &str, value: u64| {
                if value != 0 {
                    counters.insert(format!("{family}:{key}"), value);
                }
            };
            for class in TELEMETRY_CLASSES {
                let name = class_name(class);
                let offset = class_offset(class);
                insert("calls", name, fixed[offset]);
                for index in 0..13 {
                    insert(
                        "duration",
                        &format!("{name}:{index}"),
                        fixed[offset + 1 + index],
                    );
                }
                insert("near_misses", name, fixed[offset + 14]);
                insert("breaches", &format!("{name}:hard"), fixed[offset + 15]);
                insert("breaches", &format!("{name}:soft"), fixed[offset + 16]);
            }
            for (index, family) in ["queue_depth", "quota_rejections", "cpu_ms", "memory_peak"]
                .iter()
                .enumerate()
            {
                insert(family, "", fixed[255 + index]);
            }
            for index in 0..13 {
                insert(
                    "helper_round_trips",
                    &format!(":{index}"),
                    fixed[259 + index],
                );
            }
            for (index, slot) in self.names.iter().enumerate() {
                if let Some((family, name)) = slot.name() {
                    insert(NAMED_FAMILIES[family], &name, named[index]);
                }
            }
            for (family, name) in NAMED_FAMILIES.iter().enumerate() {
                insert(name, "__overflow__", named[NAME_SLOTS + family]);
            }
            let view = self.render_name.name().map(|(_, name)| name);
            for index in 0..13 {
                if let Some(view) = &view {
                    insert(
                        "render_ms",
                        &format!("{view}:{index}"),
                        named[NAME_SLOTS + 6 + index],
                    );
                }
                insert(
                    "render_ms",
                    &format!("__overflow__:{index}"),
                    named[NAME_SLOTS + 19 + index],
                );
            }
            result.push(HourBucket {
                hour_start_ms: hour.hour.saturating_mul(HOUR_MS),
                counters,
            });
        }
        result.sort_by_key(|bucket| bucket.hour_start_ms);
        result
    }

    /// Actual refused recording attempts (unprepared clock, expired observations).
    /// These counters count retries as attempts, not successfully retained observations.
    pub fn refusal_counts(&self) -> (u64, u64) {
        (
            self.clock_refusals.load(Ordering::Acquire),
            self.expired_refusals.load(Ordering::Acquire),
        )
    }

    /// Actual inline allocation size; there are no owned heap allocations in the counter store.
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self)
    }

    /// Name of the visible series for observations whose name could not be retained.
    /// This includes concurrent registration still in progress; numeric totals are preserved.
    pub fn overflow_series_name() -> &'static str {
        "__overflow__"
    }

    /// One lifetime call-count summary; the host writes this returned line to the plugin log.
    pub fn end_instance(&self, _now_ms: u64) -> Option<String> {
        if self.ended.swap(true, Ordering::AcqRel) {
            return None;
        }
        let calls = self.lifetime_calls.load(Ordering::Acquire);
        (calls != 0).then(|| format!("instance ended: {calls} calls"))
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
pub struct TelemetryRegistry {
    sides: std::sync::RwLock<BTreeMap<(String, &'static str), std::sync::Arc<SideTelemetry>>>,
}

impl TelemetryRegistry {
    /// An empty registry. Registry lookup is outside the lock-free recording path.
    pub fn new() -> Self {
        Self::default()
    }

    /// The telemetry of one admitted plugin side, created on first use.
    /// Production callers must prepare its clock before recording outside the initial window.
    pub fn side(&self, plugin: &str, side: Side) -> std::sync::Arc<SideTelemetry> {
        let mut sides = self
            .sides
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sides
            .entry((plugin.to_owned(), side.as_str()))
            .or_insert_with(|| std::sync::Arc::new(SideTelemetry::new()))
            .clone()
    }

    /// Prepares an existing side's next hour after the host releases all recording handles.
    /// The registry lock is not on the recording path. A live handle refuses maintenance
    /// rather than blocking its writer or resetting its counters underneath it.
    pub fn advance_side_clock(
        &self,
        plugin: &str,
        side: Side,
        now: TelemetryTime,
    ) -> Result<(), TelemetryError> {
        let mut sides = self
            .sides
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(telemetry) = sides.get_mut(&(plugin.to_owned(), side.as_str())) else {
            return Ok(());
        };
        let Some(telemetry) = std::sync::Arc::get_mut(telemetry) else {
            return Err(TelemetryError { code: "telemetry.handles-active".into(),
                message: "release recording handles before advancing the telemetry clock; retain and retry unprepared observations".into() });
        };
        telemetry.advance_clock(now);
        Ok(())
    }

    /// Raw counters are a host-only surface, including when a plugin asks about itself.
    pub fn read(
        &self,
        reader: &Reader,
        plugin: &str,
        side: Side,
        now_ms: u64,
    ) -> Result<Vec<HourBucket>, TelemetryError> {
        if matches!(reader, Reader::Plugin(_)) {
            return Err(TelemetryError {
                code: "not-permitted".into(),
                message: "raw telemetry is available only to the kernel, CLI and user interface"
                    .into(),
            });
        }
        let sides = self
            .sides
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(sides
            .get(&(plugin.to_owned(), side.as_str()))
            .map(|side| side.snapshot(now_ms))
            .unwrap_or_default())
    }

    /// Self summaries are allowed; another plugin requires the effective `plugins.manage` grant.
    pub fn plugin_info(
        &self,
        reader_plugin: &str,
        target: &str,
        has_manage: bool,
        now_ms: u64,
    ) -> Result<Summary, TelemetryError> {
        if reader_plugin != target && !has_manage {
            return Err(TelemetryError {
                code: "not-permitted".into(),
                message: "another plugin's summary requires plugins.manage".into(),
            });
        }
        let sides = self
            .sides
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut summary = Summary::default();
        for ((plugin, _), side) in sides.iter() {
            if plugin != target {
                continue;
            }
            for hour in side.snapshot(now_ms) {
                for class in TELEMETRY_CLASSES {
                    summary.calls = summary.calls.saturating_add(hour.calls(class));
                    summary.near_misses =
                        summary.near_misses.saturating_add(hour.near_misses(class));
                }
            }
        }
        Ok(summary)
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
    plugin: &str,
    side: Side,
    id: &str,
    detail: &str,
) -> Result<HostLogLine, TelemetryError> {
    ilium_plugin_api::ids::parse_plugin_id(plugin).map_err(|_| TelemetryError {
        code: "telemetry.plugin-invalid".into(),
        message: "Invalid plugin identity".into(),
    })?;
    let level = condition_level(id).ok_or_else(|| TelemetryError {
        code: "telemetry.condition-unknown".into(),
        message: "Unknown host condition".into(),
    })?;
    Ok(HostLogLine {
        plugin: plugin.into(),
        side,
        id: id.into(),
        level: level.into(),
        detail: truncate_message(detail),
    })
}
fn condition_level(id: &str) -> Option<&'static str> {
    match id {
        "instance.started" | "instance.ended" | "host.slow-calls" => Some("info"),
        "trap.deadline"
        | "host.late"
        | "host.queue-timeout"
        | "events.dropped"
        | "limits.clamped"
        | "deactivate.deadline" => Some("warn"),
        "host.trap" | "helper.died" => Some("error"),
        _ => None,
    }
}
#[derive(Debug, Default)]
struct HostLogWindow {
    start: u64,
    admitted: u32,
    suppressed: u64,
    pending: u64,
}
/// One plugin's host-line limiter: twenty lines per condition, side and minute.
#[derive(Debug, Default)]
pub struct HostLogLimiter {
    windows: BTreeMap<(&'static str, String), HostLogWindow>,
}
impl HostLogLimiter {
    /// An empty per-plugin limiter.
    pub fn new() -> Self {
        Self::default()
    }
    /// Unknown conditions fail closed and do not allocate cardinality.
    pub fn admit(&mut self, side: Side, id: &str, now_ms: u64) -> bool {
        if condition_level(id).is_none() {
            return false;
        }
        let window = self
            .windows
            .entry((side.as_str(), id.into()))
            .or_insert(HostLogWindow {
                start: now_ms,
                ..HostLogWindow::default()
            });
        advance_host_window(window, now_ms);
        if window.admitted >= 20 {
            window.suppressed = window.suppressed.saturating_add(1);
            return false;
        }
        window.admitted += 1;
        true
    }
    /// One actual suppression summary, outside the admission budget.
    pub fn take_suppressed_summary(&mut self, side: Side, id: &str, now_ms: u64) -> Option<String> {
        let window = self.windows.get_mut(&(side.as_str(), id.into()))?;
        advance_host_window(window, now_ms);
        let count = std::mem::take(&mut window.pending);
        (count > 0).then(|| format!("{count} similar lines suppressed"))
    }
}
fn advance_host_window(window: &mut HostLogWindow, now_ms: u64) {
    if now_ms.saturating_sub(window.start) >= 60_000 {
        window.pending = window
            .pending
            .saturating_add(std::mem::take(&mut window.suppressed));
        window.start = now_ms;
        window.admitted = 0;
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

#[derive(Debug, Default)]
struct SlowWindow {
    start: u64,
    top: Vec<(CallClass, Duration)>,
    eligible: bool,
}
/// One plugin's per-side top-three collector. The host drives the minute tick.
#[derive(Debug, Default)]
pub struct SlowCallReporter {
    windows: BTreeMap<&'static str, SlowWindow>,
}
impl SlowCallReporter {
    /// An empty reporter.
    pub fn new() -> Self {
        Self::default()
    }
    /// Samples every call, but a report requires at least one above 50 percent.
    pub fn record(
        &mut self,
        side: Side,
        class: CallClass,
        took: Duration,
        deadline: Duration,
        now_ms: u64,
    ) {
        let window = self.windows.entry(side.as_str()).or_insert(SlowWindow {
            start: now_ms,
            ..SlowWindow::default()
        });
        window.eligible |= took.as_nanos() * 2 > deadline.as_nanos();
        window.top.push((class, took));
        window.top.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        window.top.truncate(3);
    }
    /// Returns actual samples once per elapsed minute; no empty report is invented.
    pub fn due_report(&mut self, side: Side, now_ms: u64) -> Option<SlowCallReport> {
        let window = self.windows.get_mut(side.as_str())?;
        if now_ms.saturating_sub(window.start) < 60_000 {
            return None;
        }
        window.start = now_ms;
        let top = std::mem::take(&mut window.top);
        let eligible = std::mem::take(&mut window.eligible);
        (eligible && !top.is_empty()).then(|| SlowCallReport {
            id: "host.slow-calls".into(),
            top,
        })
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
    serializer.serialize_u64(u64::try_from(duration.as_micros()).unwrap_or(u64::MAX))
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

/// Records spans only when `ILIUM_PLUGIN_TRACE=1` (reference-host-telemetry.md HST-360).
#[derive(Debug, Default)]
pub struct SpanRecorder {
    enabled: bool,
    spans: Vec<Span>,
    dropped: u64,
}

impl SpanRecorder {
    /// A recorder configured from the value of `ILIUM_PLUGIN_TRACE`.
    pub fn from_env(value: Option<&str>) -> Self {
        Self {
            enabled: value == Some("1"),
            ..Self::default()
        }
    }

    /// Whether spans are recorded.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Records one span when enabled.
    pub fn record(&mut self, span: Span) {
        if !self.enabled {
            return;
        }
        if self.spans.len() == TRACE_RING_CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.spans.push(span);
    }
    /// Drains actual captured spans into the host trace sink.
    pub fn take_spans(&mut self) -> Vec<Span> {
        std::mem::take(&mut self.spans)
    }
    /// Spans refused because the host did not drain the bounded capture buffer.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The recorded spans, in order.
    pub fn spans(&self) -> &[Span] {
        &self.spans
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
pub fn why_lines(inputs: &HealthInputs) -> Vec<String> {
    let mut lines = Vec::new();
    if inputs.near_misses >= 5 {
        lines.push(format!(
            "Why: close to deadline: {} near-misses in {} calls",
            inputs.near_misses, inputs.calls
        ));
    }
    if inputs.quota_rejections_last_minute > 0 {
        lines.push(format!(
            "Why: held by quota: {} quota rejections in the last minute",
            inputs.quota_rejections_last_minute
        ));
    }
    if inputs.memory_limit_bytes > 0
        && u128::from(inputs.memory_peak_bytes) * 10 > u128::from(inputs.memory_limit_bytes) * 9
    {
        lines.push(format!(
            "Why: memory near the limit: {} of {} bytes",
            inputs.memory_peak_bytes, inputs.memory_limit_bytes
        ));
    }
    lines
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
pub fn plugin_log_dir(state_dir: &Path, project: &Path, session: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for bytes in [project.as_os_str().as_encoded_bytes(), session.as_bytes()] {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    let readable: String = session
        .chars()
        .take(32)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect();
    state_dir
        .join("plugins")
        .join("logs")
        .join(format!("{readable}-{:x}", digest.finalize()))
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
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
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
        match self {
            Self::Server => "server",
            Self::Client => "client",
            Self::Scene => "scene",
            Self::Host => "host",
        }
    }
}

/// One formatted log line with the fixed columns (DEV-650).
pub fn format_log_line(
    at: &str,
    level: LogLevel,
    side: LogSide,
    plugin: &str,
    message: &str,
) -> String {
    let message = message.replace('\r', "\\r").replace('\n', "\\n");
    format!(
        "{at} {:<5} {:<6} {plugin} {}",
        level.as_str(),
        side.as_str(),
        truncate_message(&message)
    )
}

/// The longest message kept in a log line, in bytes (DEV-651).
pub const LOG_MESSAGE_MAX_BYTES: usize = 16_384;

/// The size at which a log file rotates (DEV-657).
pub const LOG_ROTATE_BYTES: u64 = 4 * 1024 * 1024;

/// Cuts a message to `LOG_MESSAGE_MAX_BYTES` on a character boundary (DEV-651).
pub fn truncate_message(message: &str) -> String {
    cut_message(message, LOG_MESSAGE_MAX_BYTES, "…[truncated]")
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
            rate_per_second: 200,
        }
    }
}

impl LogConfig {
    /// The configuration of a dev install: `debug` threshold (DEV-653).
    pub fn for_dev_install() -> LogConfig {
        Self {
            threshold: LogLevel::Debug,
            ..Self::default()
        }
    }

    /// The configuration for a plugin of a tier (DEV-653).
    pub fn for_tier(tier: Tier) -> LogConfig {
        Self {
            rate_per_second: if tier == Tier::T1 { 1000 } else { 200 },
            ..Self::default()
        }
    }
}

fn log_refusal(code: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code: code.into(),
        message: message.into(),
    }
}

fn log_io(error: std::io::Error) -> Refusal {
    log_refusal("log.io", error.to_string())
}

fn check_plugin_log_id(plugin: &str) -> Result<(), Refusal> {
    ilium_plugin_api::ids::parse_plugin_id(plugin)
        .map(|_| ())
        .map_err(|_| log_refusal("log.plugin-id", "invalid plugin identifier"))
}

fn check_directory_ancestry(dir: &Path) -> Result<(), Refusal> {
    let mut current = PathBuf::new();
    for component in dir.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(log_refusal(
                "log.path",
                "parent traversal is not a private directory",
            ));
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(log_refusal(
                    "log.path",
                    "directory ancestry must not contain links or files",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(log_io(error)),
        }
    }
    Ok(())
}

fn private_log_directory(dir: &Path) -> Result<(), Refusal> {
    if !dir.is_absolute() || dir.parent().is_none() {
        return Err(log_refusal(
            "log.path",
            "a nested absolute private directory is required",
        ));
    }
    // Refuse parent escapes and existing symlinks at every ancestor, not just the log leaf.
    let mut current = PathBuf::new();
    for component in dir.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(log_refusal(
                "log.path",
                "parent traversal is not a log directory",
            ));
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(log_refusal(
                    "log.path",
                    "log ancestry must contain only real directories",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ilium_platform::secure_fs::create_private_directory(&current).map_err(log_io)?
            }
            Err(error) => return Err(log_io(error)),
        }
    }
    ilium_platform::secure_fs::restrict_directory_to_owner(dir).map_err(log_io)
}

fn private_log_file(path: &Path) -> Result<std::fs::File, Refusal> {
    let file = ilium_platform::secure_fs::private_open_options()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(log_io)?;
    if !file.metadata().map_err(log_io)?.is_file() {
        return Err(log_refusal(
            "log.path",
            "log storage must be a regular file",
        ));
    }
    ilium_platform::secure_fs::restrict_open_file_to_owner(&file).map_err(log_io)?;
    Ok(file)
}

fn read_private_log_file(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, Refusal> {
    use std::io::Read;
    let parent = path
        .parent()
        .ok_or_else(|| log_refusal("log.path", "private file needs a parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| log_refusal("log.path", "private file needs a leaf"))?;
    let directory =
        ilium_platform::secure_fs::NoFollowDirectory::open_root(parent).map_err(log_io)?;
    let file = match directory.open_regular(name) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(log_io(error)),
    };
    if file.metadata().map_err(log_io)?.len() > limit {
        return Err(log_refusal(
            "log.state-invalid",
            "log state exceeds its byte limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(log_io)?;
    if bytes.len() as u64 > limit {
        return Err(log_refusal(
            "log.state-invalid",
            "log state grew beyond its limit",
        ));
    }
    Ok(Some(bytes))
}

fn write_private_log_state(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    use std::io::Write;
    check_log_leaf(path)?;
    let temporary = path.with_extension("tmp");
    let mut file = private_log_file(&temporary)?;
    file.set_len(0).map_err(log_io)?;
    file.write_all(bytes).map_err(log_io)?;
    file.sync_all().map_err(log_io)?;
    ilium_platform::secure_fs::replace_file_durably(&temporary, path).map_err(log_io)
}

fn check_log_leaf(path: &Path) -> Result<bool, Refusal> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(log_refusal(
            "log.path",
            "log leaf must be a regular file, never a link",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(log_io(error)),
    }
}

fn valid_log_time(at: &str) -> bool {
    let bytes = at.as_bytes();
    if bytes.len() != 19 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let delimiter = match index {
            4 | 7 => Some(b'-'),
            10 => Some(b' '),
            13 | 16 => Some(b':'),
            _ => None,
        };
        if delimiter.map_or(!byte.is_ascii_digit(), |expected| *byte != expected) {
            return false;
        }
    }
    let parse = |range: std::ops::Range<usize>| at[range].parse::<u32>().unwrap_or(u32::MAX);
    let year = parse(0..4);
    let month = parse(5..7);
    let day = parse(8..10);
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                29
            } else {
                28
            }
        }
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0
        && (1..=days).contains(&day)
        && parse(11..13) < 24
        && parse(14..16) < 60
        && parse(17..19) < 60
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginLogRate {
    window_start_ms: Option<u64>,
    admitted: u32,
    dropped: u64,
}

/// Fixed-memory rate accounting for one actual admitted runtime instance.
/// Share this object across all of that instance's log import handles. The host
/// must not recreate it to reopen quota; a new instance receives a new budget.
#[derive(Debug)]
pub struct PluginLogInstanceBudget {
    instance: u64,
    side: Side,
    rate: PluginLogRate,
}

impl PluginLogInstanceBudget {
    pub fn new(actual_instance: u64, side: Side) -> Result<Self, Refusal> {
        if actual_instance == 0 {
            return Err(log_refusal(
                "log.instance-invalid",
                "an actual nonzero runtime instance identity is required",
            ));
        }
        Ok(Self {
            instance: actual_instance,
            side,
            rate: PluginLogRate::default(),
        })
    }

    pub fn instance(&self) -> u64 {
        self.instance
    }

    pub fn side(&self) -> Side {
        self.side
    }
}

/// A private, cross-process serialized plugin log. Files are reopened under the lock so
/// rotation by another writer never leaves this writer appending to an archived inode.
/// The host supplies real local timestamps and one shared monotonic clock origin.
/// Ordinary messages written by plugins may contain intentional data; host callers must
/// supply metadata-only messages. Installed secret variants are redacted from both paths.
pub struct PluginLog {
    dir: PathBuf,
    plugin: String,
    config: LogConfig,
    secret_variants: Vec<String>,
    last_error: Option<Refusal>,
}

impl std::fmt::Debug for PluginLog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginLog")
            .field("plugin", &self.plugin)
            .field("config", &self.config)
            .field("last_error", &self.last_error)
            .finish_non_exhaustive()
    }
}

impl PluginLog {
    /// Opens the actual log. `config` must come from admitted tier/settings context.
    pub fn open(dir: &Path, plugin: &str, config: LogConfig) -> Result<Self, Refusal> {
        check_plugin_log_id(plugin)?;
        if config.rate_per_second > 1000 {
            return Err(log_refusal(
                "log.rate-invalid",
                "plugin log rate cannot exceed the trusted-tier ceiling",
            ));
        }
        private_log_directory(dir)?;
        let log = Self {
            dir: dir.to_owned(),
            plugin: plugin.into(),
            config,
            secret_variants: Vec::new(),
            last_error: None,
        };
        let _lock = log.lock()?;
        for index in 0..5 {
            check_log_leaf(&log.path(index))?;
        }
        private_log_file(&log.path(0))?;
        log.rate()?;
        Ok(log)
    }

    /// Supplies current effective secrets before accepting logs. Debug output never prints them.
    pub fn set_secrets(&mut self, secrets: &[String]) {
        self.secret_variants = crate::secrets::variants(secrets);
    }

    /// Last persistence/clock/format refusal; threshold/rate drops are not I/O errors.
    pub fn take_error(&mut self) -> Option<Refusal> {
        self.last_error.take()
    }

    fn path(&self, index: usize) -> PathBuf {
        self.dir.join(if index == 0 {
            format!("{}.log", self.plugin)
        } else {
            format!("{}.log.{index}", self.plugin)
        })
    }
    fn state_path(&self) -> PathBuf {
        self.dir.join(format!(".{}.log-rate.json", self.plugin))
    }
    fn lock(&self) -> Result<ilium_platform::file_lock::ExclusiveFileLock, Refusal> {
        private_log_directory(&self.dir)?;
        ilium_platform::file_lock::ExclusiveFileLock::acquire(
            &self.dir.join(format!(".{}.log.lock", self.plugin)),
        )
        .map_err(log_io)
    }
    fn rate(&self) -> Result<PluginLogRate, Refusal> {
        let Some(bytes) = read_private_log_file(&self.state_path(), 4096)? else {
            return Ok(PluginLogRate::default());
        };
        let rate: PluginLogRate = serde_json::from_slice(&bytes).map_err(|_| {
            log_refusal(
                "log.state-invalid",
                "invalid rate state; original bytes preserved",
            )
        })?;
        if rate.admitted > 1000
            || (rate.window_start_ms.is_none() && (rate.admitted != 0 || rate.dropped != 0))
        {
            return Err(log_refusal("log.state-invalid", "invalid rate counters"));
        }
        Ok(rate)
    }
    fn save_rate(&self, rate: &PluginLogRate) -> Result<(), Refusal> {
        let bytes = serde_json::to_vec(rate)
            .map_err(|_| log_refusal("log.state-invalid", "cannot encode rate state"))?;
        write_private_log_state(&self.state_path(), &bytes)
    }
    fn append(
        &self,
        at: &str,
        level: LogLevel,
        side: LogSide,
        message: &str,
    ) -> Result<(), Refusal> {
        use std::io::{Read, Seek, Write};
        if !valid_log_time(at) {
            return Err(log_refusal(
                "log.time-invalid",
                "a real local timestamp YYYY-MM-DD HH:mm:ss is required",
            ));
        }
        for index in 0..5 {
            check_log_leaf(&self.path(index))?;
        }
        let message = crate::secrets::redact_variants(message, &self.secret_variants);
        let line = format_log_line(at, level, side, &self.plugin, &message);
        let mut file = private_log_file(&self.path(0))?;
        let mut previous_last_byte = [b'\n'];
        if file.metadata().map_err(log_io)?.len() != 0 {
            file.seek(std::io::SeekFrom::End(-1)).map_err(log_io)?;
            file.read_exact(&mut previous_last_byte).map_err(log_io)?;
        }
        file.seek(std::io::SeekFrom::End(0)).map_err(log_io)?;
        // Preserve a partial prior append verbatim, but do not concatenate a new entry to it.
        if previous_last_byte[0] != b'\n' {
            file.write_all(b"\n").map_err(log_io)?;
        }
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(log_io)?;
        file.sync_all().map_err(log_io)?;
        if file.metadata().map_err(log_io)?.len() >= LOG_ROTATE_BYTES {
            drop(file);
            if check_log_leaf(&self.path(4))? {
                std::fs::remove_file(self.path(4)).map_err(log_io)?;
            }
            for index in (0..4).rev() {
                if check_log_leaf(&self.path(index))? {
                    std::fs::rename(self.path(index), self.path(index + 1)).map_err(log_io)?;
                }
            }
            private_log_file(&self.path(0))?
                .sync_all()
                .map_err(log_io)?;
            ilium_platform::secure_fs::sync_parent_directory_if_supported(&self.path(0))
                .map_err(log_io)?;
        }
        Ok(())
    }
    fn advance_rate_window(
        &self,
        at: &str,
        now_ms: u64,
        rate: &mut PluginLogRate,
    ) -> Result<bool, Refusal> {
        let Some(start) = rate.window_start_ms else {
            rate.window_start_ms = Some(now_ms);
            return Ok(false);
        };
        if now_ms < start {
            return Err(log_refusal(
                "log.clock-regressed",
                "log rate clock origin changed or regressed",
            ));
        }
        if now_ms - start < 1000 {
            return Ok(false);
        }
        if rate.dropped != 0 {
            self.append(
                at,
                LogLevel::Warn,
                LogSide::Host,
                &format!("{} lines dropped in the last second", rate.dropped),
            )?;
        }
        *rate = PluginLogRate {
            window_start_ms: Some(now_ms),
            ..PluginLogRate::default()
        };
        Ok(true)
    }

    fn advance_rate(&self, at: &str, now_ms: u64, rate: &mut PluginLogRate) -> Result<(), Refusal> {
        if self.advance_rate_window(at, now_ms, rate)? {
            self.save_rate(rate)?;
        }
        Ok(())
    }

    /// Uses the caller's instance-owned budget, never the legacy persisted rate.
    /// Reserve admission before appending: an I/O failure does not reopen quota.
    pub fn write_plugin_for_instance(
        &mut self,
        at: &str,
        now_ms: u64,
        budget: &mut PluginLogInstanceBudget,
        level: LogLevel,
        message: &str,
    ) -> bool {
        if level < self.config.threshold {
            return false;
        }
        let result = (|| {
            if !valid_log_time(at) {
                return Err(log_refusal(
                    "log.time-invalid",
                    "a real local timestamp YYYY-MM-DD HH:mm:ss is required",
                ));
            }
            let _lock = self.lock()?;
            self.advance_rate_window(at, now_ms, &mut budget.rate)?;
            if budget.rate.admitted >= self.config.rate_per_second {
                budget.rate.dropped = budget.rate.dropped.saturating_add(1);
                return Ok(false);
            }
            budget.rate.admitted = budget.rate.admitted.saturating_add(1);
            let side = match budget.side {
                Side::Server => LogSide::Server,
                Side::Client => LogSide::Client,
                Side::Scene => LogSide::Scene,
            };
            self.append(at, level, side, message)?;
            Ok(true)
        })();
        match result {
            Ok(written) => written,
            Err(error) => {
                self.last_error = Some(error);
                false
            }
        }
    }

    /// Flushes a due instance suppression summary without consuming guest quota.
    /// The owning runtime must retain and tick the budget through final draining.
    pub fn tick_instance(
        &mut self,
        at: &str,
        now_ms: u64,
        budget: &mut PluginLogInstanceBudget,
    ) {
        let result = (|| {
            if !valid_log_time(at) {
                return Err(log_refusal(
                    "log.time-invalid",
                    "a real local timestamp YYYY-MM-DD HH:mm:ss is required",
                ));
            }
            let _lock = self.lock()?;
            self.advance_rate_window(at, now_ms, &mut budget.rate)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.last_error = Some(error);
        }
    }

    /// Writes a host metadata line, outside the guest line budget.
    pub fn write_host(&mut self, at: &str, _now_ms: u64, level: LogLevel, message: &str) -> bool {
        if level < self.config.threshold {
            return false;
        }
        let result = (|| {
            let _lock = self.lock()?;
            self.append(at, level, LogSide::Host, message)
        })();
        match result {
            Ok(()) => true,
            Err(error) => {
                self.last_error = Some(error);
                false
            }
        }
    }

    /// Writes an admitted guest line; the enum cannot impersonate the host side.
    pub fn write_plugin(
        &mut self,
        at: &str,
        now_ms: u64,
        side: Side,
        level: LogLevel,
        message: &str,
    ) -> bool {
        if level < self.config.threshold {
            return false;
        }
        if !valid_log_time(at) {
            self.last_error = Some(log_refusal(
                "log.time-invalid",
                "a real local timestamp YYYY-MM-DD HH:mm:ss is required",
            ));
            return false;
        }
        let result = (|| {
            let _lock = self.lock()?;
            let mut rate = self.rate()?;
            self.advance_rate(at, now_ms, &mut rate)?;
            if rate.admitted >= self.config.rate_per_second {
                rate.dropped = rate.dropped.saturating_add(1);
                self.save_rate(&rate)?;
                return Ok(false);
            }
            rate.admitted = rate.admitted.saturating_add(1);
            // Reserve capacity durably before the write. Failed writes never reopen quota.
            self.save_rate(&rate)?;
            self.append(
                at,
                level,
                match side {
                    Side::Server => LogSide::Server,
                    Side::Client => LogSide::Client,
                    Side::Scene => LogSide::Scene,
                },
                message,
            )?;
            Ok(true)
        })();
        match result {
            Ok(written) => written,
            Err(error) => {
                self.last_error = Some(error);
                false
            }
        }
    }

    /// Flushes a due suppression summary; the durable state survives close/reopen.
    pub fn tick(&mut self, at: &str, now_ms: u64) {
        let result = (|| {
            let _lock = self.lock()?;
            let mut rate = self.rate()?;
            self.advance_rate(at, now_ms, &mut rate)
        })();
        if let Err(error) = result {
            self.last_error = Some(error);
        }
    }

    /// Rebinds rate accounting after the host has established a genuinely new clock epoch
    /// and drained every old writer. This is never inferred from a backwards timestamp.
    /// Pending loss is reported before resetting the old epoch's budget.
    pub fn begin_rate_clock_epoch(&mut self, at: &str, now_ms: u64) -> Result<(), Refusal> {
        let _lock = self.lock()?;
        let rate = self.rate()?;
        if rate.dropped != 0 {
            self.append(
                at,
                LogLevel::Warn,
                LogSide::Host,
                &format!("{} lines dropped in the last second", rate.dropped),
            )?;
        }
        self.save_rate(&PluginLogRate {
            window_start_ms: Some(now_ms),
            ..PluginLogRate::default()
        })
    }

    /// Writes the exact host queue-overflow report without consuming guest quota.
    pub fn queue_full(&mut self, at: &str, now_ms: u64, dropped: u64) {
        self.write_host(
            at,
            now_ms,
            LogLevel::Error,
            &format!("event queue full: dropped {dropped} events"),
        );
    }
}

/// The effective threshold from a flag, the settings value and the dev flag (DEV-653).
pub fn resolve_threshold(
    flag: Option<LogLevel>,
    setting: Option<&str>,
    dev_install: bool,
) -> LogLevel {
    flag.or_else(|| {
        setting.and_then(|text| {
            LogLevel::ALL
                .into_iter()
                .find(|level| level.as_str() == text)
        })
    })
    .unwrap_or(if dev_install {
        LogLevel::Debug
    } else {
        LogLevel::Info
    })
}

/// The level and text of the restart line (DEV-659).
pub fn restart_log(delay_s: u64, failure: u32, of: u32) -> (LogLevel, String) {
    (
        LogLevel::Warn,
        format!("restarting in {delay_s} s (failure {failure} of {of} in 10 minutes)"),
    )
}

/// The level and text of the disabled-after-failures line (DEV-659).
pub fn disabled_log(failures: u32) -> (LogLevel, String) {
    (
        LogLevel::Error,
        format!("disabled after {failures} failures in 10 minutes"),
    )
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LogUninstallMark {
    at_unix_millis: u64,
}

fn uninstall_mark_path(dir: &Path, plugin: &str) -> PathBuf {
    dir.join(format!(".{plugin}.log-uninstalled.json"))
}

fn retention_lock(
    dir: &Path,
    plugin: &str,
) -> Result<ilium_platform::file_lock::ExclusiveFileLock, Refusal> {
    check_plugin_log_id(plugin)?;
    private_log_directory(dir)?;
    ilium_platform::file_lock::ExclusiveFileLock::acquire(&dir.join(format!(".{plugin}.log.lock")))
        .map_err(log_io)
}

/// Persists the actual uninstall time in Unix milliseconds; no process-clock value may be used.
/// A later reinstall must clear this mark under the same lock before starting its instance.
pub fn try_mark_uninstalled(
    log_dir: &Path,
    plugin: &str,
    at_unix_millis: u64,
) -> Result<(), Refusal> {
    let _lock = retention_lock(log_dir, plugin)?;
    let bytes = serde_json::to_vec(&LogUninstallMark { at_unix_millis })
        .map_err(|_| log_refusal("log.state-invalid", "cannot encode uninstall time"))?;
    write_private_log_state(&uninstall_mark_path(log_dir, plugin), &bytes)
}

/// Compatibility entry point; failures are visible on stderr. Hosts should use the checked method.
pub fn mark_uninstalled(log_dir: &Path, plugin: &str, now_ms: u64) {
    if let Err(error) = try_mark_uninstalled(log_dir, plugin, now_ms) {
        eprintln!(
            "plugin log retention refused [{}]: {}",
            error.code, error.message
        );
    }
}

/// Clears an uninstall mark without deleting logs or rewriting its timestamp.
pub fn try_mark_reinstalled(log_dir: &Path, plugin: &str) -> Result<(), Refusal> {
    let _lock = retention_lock(log_dir, plugin)?;
    let path = uninstall_mark_path(log_dir, plugin);
    if check_log_leaf(&path)? {
        std::fs::remove_file(&path).map_err(log_io)?;
        ilium_platform::secure_fs::sync_parent_directory_if_supported(&path).map_err(log_io)?;
    }
    Ok(())
}

/// Compatibility entry point; hosts should retain checked failure diagnostics.
pub fn mark_reinstalled(log_dir: &Path, plugin: &str) {
    if let Err(error) = try_mark_reinstalled(log_dir, plugin) {
        eprintln!(
            "plugin log retention refused [{}]: {}",
            error.code, error.message
        );
    }
}

fn delete_plugin_logs_locked(log_dir: &Path, plugin: &str) -> Result<(), Refusal> {
    let mut paths: Vec<PathBuf> = (0..5)
        .map(|index| {
            log_dir.join(if index == 0 {
                format!("{plugin}.log")
            } else {
                format!("{plugin}.log.{index}")
            })
        })
        .collect();
    paths.push(log_dir.join(format!(".{plugin}.log-rate.json")));
    paths.push(log_dir.join(format!(".{plugin}.log-rate.tmp")));
    // Validate the whole exact deletion set before mutating any of it.
    for path in &paths {
        check_log_leaf(path)?;
    }
    for path in paths {
        if check_log_leaf(&path)? {
            std::fs::remove_file(path).map_err(log_io)?;
        }
    }
    let mark = uninstall_mark_path(log_dir, plugin);
    if check_log_leaf(&mark)? {
        std::fs::remove_file(&mark).map_err(log_io)?;
    }
    ilium_platform::secure_fs::sync_parent_directory_if_supported(&mark).map_err(log_io)?;
    Ok(())
}

/// Purges only the five documented files and private state of this admitted plugin.
/// The lock inode is retained: removing it would split custody between concurrent processes.
pub fn purge_plugin_logs(log_dir: &Path, plugin: &str) -> Result<(), Refusal> {
    let _lock = retention_lock(log_dir, plugin)?;
    delete_plugin_logs_locked(log_dir, plugin)
}

/// Actual results of a retention pass, including independent failures.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogRetentionReport {
    /// Plugin ids whose documented log files were removed.
    pub removed: Vec<String>,
    /// Refusals that preserved the affected plugin's remaining files.
    pub errors: Vec<Refusal>,
}

fn purge_one_expired(log_dir: &Path, plugin: &str, now_unix_millis: u64) -> Result<bool, Refusal> {
    check_plugin_log_id(plugin)?;
    let _lock = retention_lock(log_dir, plugin)?;
    let Some(bytes) = read_private_log_file(&uninstall_mark_path(log_dir, plugin), 4096)? else {
        return Ok(false);
    };
    let mark: LogUninstallMark = serde_json::from_slice(&bytes).map_err(|_| {
        log_refusal(
            "log.state-invalid",
            "invalid uninstall mark; logs preserved",
        )
    })?;
    if now_unix_millis
        .checked_sub(mark.at_unix_millis)
        .is_none_or(|elapsed| elapsed < 7 * 24 * HOUR_MS)
    {
        return Ok(false);
    }
    delete_plugin_logs_locked(log_dir, plugin)?;
    Ok(true)
}

/// Checks every mark; one malformed plugin mark cannot indefinitely prevent other expiries.
/// Invalid mark bytes and unrelated directory entries are preserved.
pub fn purge_expired_logs_report(
    log_dir: &Path,
    now_unix_millis: u64,
) -> Result<LogRetentionReport, Refusal> {
    if !log_dir.try_exists().map_err(log_io)? {
        return Ok(LogRetentionReport::default());
    }
    private_log_directory(log_dir)?;
    let mut report = LogRetentionReport::default();
    for entry in std::fs::read_dir(log_dir).map_err(log_io)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.errors.push(log_io(error));
                continue;
            }
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(plugin) = name
            .strip_prefix('.')
            .and_then(|name| name.strip_suffix(".log-uninstalled.json"))
        else {
            continue;
        };
        match purge_one_expired(log_dir, plugin, now_unix_millis) {
            Ok(true) => report.removed.push(plugin.to_owned()),
            Ok(false) => {}
            Err(mut error) => {
                error.message = format!("{plugin}: {}", error.message);
                report.errors.push(error);
            }
        }
    }
    report.removed.sort();
    Ok(report)
}

/// Checked retention convenience. Use the report method when partial progress must be displayed.
pub fn try_purge_expired_logs(
    log_dir: &Path,
    now_unix_millis: u64,
) -> Result<Vec<String>, Refusal> {
    let report = purge_expired_logs_report(log_dir, now_unix_millis)?;
    match report.errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(report.removed),
    }
}

/// Compatibility entry point; hosts should use the checked method to surface retention failures.
pub fn purge_expired_logs(log_dir: &Path, now_ms: u64) {
    if let Err(error) = try_purge_expired_logs(log_dir, now_ms) {
        eprintln!(
            "plugin log retention refused [{}]: {}",
            error.code, error.message
        );
    }
}

/// Converts real wall time through the platform's local time zone. Errors never fabricate UTC.
pub fn try_format_local_time(time: SystemTime) -> Result<String, Refusal> {
    ilium_platform::local_time::format_timestamp(time).map_err(log_io)
}

/// Local-time column, or an explicit unavailable marker. The writer refuses that marker;
/// hosts that need the cause should use `try_format_local_time`.
pub fn format_local_time(time: SystemTime) -> String {
    try_format_local_time(time).unwrap_or_else(|_| "<local-time-unavailable>".into())
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
        match self {
            Self::Event => "event",
            Self::Submission => "submission",
            Self::Outcome => "outcome",
            Self::HostCall => "host-call",
            Self::Render => "render",
            Self::Trap => "trap",
            Self::Lifecycle => "lifecycle",
        }
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

/// A bounded ring of retained records, including prior-instance records until eviction (DEV-670, DEV-676).
#[derive(Debug, Default)]
pub struct TraceRing {
    instance: u64,
    sequence: u64,
    records: VecDeque<TraceRecord>,
}

impl TraceRing {
    /// An empty ring.
    pub fn new() -> TraceRing {
        Self::default()
    }

    /// Starts an instance without clearing retained history (DEV-676).
    /// Zero means the instance number is exhausted; no new instance was started.
    pub fn begin_instance(&mut self) -> u64 {
        let Some(next) = self.instance.checked_add(1) else {
            return 0;
        };
        self.instance = next;
        self.sequence = 0;
        next
    }

    /// Pushes a redacted record; zero means sequence/instance exhaustion.
    /// Production callers that need an explicit refusal use `try_push`.
    pub fn push(&mut self, time_ms: u64, kind: TraceKind, detail: serde_json::Value) -> u64 {
        self.try_push(time_ms, kind, detail).unwrap_or(0)
    }
    /// Structured admission, without wrapping or reusing a sequence number.
    pub fn try_push(
        &mut self,
        time_ms: u64,
        kind: TraceKind,
        detail: serde_json::Value,
    ) -> Result<u64, Refusal> {
        self.push_detail(time_ms, kind, redact_detail(&detail))
    }
    /// Explicit dev-only unredacted admission; a normal push always redacts.
    pub fn push_unredacted(
        &mut self,
        time_ms: u64,
        kind: TraceKind,
        detail: serde_json::Value,
        dev_install: bool,
    ) -> Result<u64, Refusal> {
        check_unredacted(dev_install)?;
        self.push_detail(time_ms, kind, detail)
    }
    fn push_detail(
        &mut self,
        time_ms: u64,
        kind: TraceKind,
        detail: serde_json::Value,
    ) -> Result<u64, Refusal> {
        let exhausted = || Refusal {
            code: "trace.sequence-exhausted".into(),
            message: "Trace sequence or instance identity exhausted".into(),
        };
        if self.instance == 0 && self.begin_instance() == 0 {
            return Err(exhausted());
        }
        let seq = self.sequence.checked_add(1).ok_or_else(exhausted)?;
        self.sequence = seq;
        if self.records.len() == TRACE_RING_CAPACITY {
            self.records.pop_front();
        }
        self.records.push_back(TraceRecord {
            seq,
            time_ms,
            kind,
            instance: self.instance,
            detail,
        });
        Ok(seq)
    }

    /// The records, oldest first.
    pub fn records(&self) -> Vec<TraceRecord> {
        self.records.iter().cloned().collect()
    }
}

/// Replaces every payload in a detail by its byte length, keeping the tags (DEV-672).
pub fn redact_detail(detail: &serde_json::Value) -> serde_json::Value {
    fn payload(value: &serde_json::Value) -> serde_json::Value {
        let bytes = match value {
            serde_json::Value::String(text) => text.len(),
            serde_json::Value::Array(values)
                if values
                    .iter()
                    .all(|value| value.as_u64().is_some_and(|byte| byte <= 255)) =>
            {
                values.len()
            }
            _ => serde_json::to_vec(value).map_or(0, |bytes| bytes.len()),
        };
        serde_json::Value::String(format!("<redacted {bytes} bytes>"))
    }
    fn walk(detail: &serde_json::Value, metadata: bool) -> serde_json::Value {
        match detail {
            serde_json::Value::Object(fields) => fields
                .iter()
                .map(|(key, value)| {
                    let sensitive = matches!(
                        key.as_str(),
                        "payload"
                            | "payloads"
                            | "data"
                            | "body"
                            | "content"
                            | "text"
                            | "prompt"
                            | "value"
                            | "bytes"
                            | "arguments"
                            | "path"
                            | "url"
                            | "terminal"
                    );
                    let metadata = matches!(
                        key.as_str(),
                        "tag"
                            | "tags"
                            | "name"
                            | "names"
                            | "event"
                            | "events"
                            | "intent"
                            | "kind"
                            | "correlation"
                            | "dedup"
                            | "dedup_key"
                            | "export"
                            | "code"
                            | "result"
                            | "target"
                            | "class"
                            | "reason"
                    );
                    (
                        key.clone(),
                        if sensitive {
                            payload(value)
                        } else {
                            walk(value, metadata)
                        },
                    )
                })
                .collect(),
            serde_json::Value::Array(values) => {
                values.iter().map(|value| walk(value, metadata)).collect()
            }
            serde_json::Value::String(_) if !metadata => payload(detail),
            _ => detail.clone(),
        }
    }
    walk(detail, false)
}

/// Whether `--unredacted` is allowed: only for a dev install (DEV-672).
pub fn check_unredacted(dev_install: bool) -> Result<(), Refusal> {
    if dev_install {
        Ok(())
    } else {
        Err(Refusal {
            code: "trace.unredacted-denied".into(),
            message: "Unredacted traces require a dev install".into(),
        })
    }
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

/// Renders the cached tree with its actual node/depth limits (DEV-680..682).
/// This function has no guest-render side effects.
pub fn render_inspector(
    view: &str,
    tree: &InspectNode,
    node_limit: usize,
    depth_limit: usize,
) -> String {
    render_inspector_with_limits(
        view,
        tree,
        80,
        usize::MAX,
        u64::try_from(node_limit).unwrap_or(u64::MAX),
        u32::try_from(depth_limit).unwrap_or(u32::MAX),
    )
}
/// Explicit display area plus actual resolved limits. The caller supplies the
/// last rendered tree; this renderer never changes selection or focus.
pub fn render_inspector_with_limits(
    view: &str,
    tree: &InspectNode,
    columns: usize,
    rows: usize,
    node_limit: u64,
    depth_limit: u32,
) -> String {
    if rows == 0 || columns == 0 {
        return String::new();
    }
    let mut lines = vec![format!("╭ Widget tree: {} ", line_safe(view))];
    let mut pending = vec![(tree, 1_usize)];
    let mut count = 0_u64;
    let mut depth = 0;
    while let Some((node, level)) = pending.pop() {
        count = count.saturating_add(1);
        depth = depth.max(level);
        if lines.len() < rows.saturating_sub(2) {
            let mut text = format!(
                "│ {}{}",
                "  ".repeat(level.saturating_sub(1).min(columns / 2)),
                line_safe(&node.kind)
            );
            if let Some(key) = &node.key {
                text.push_str(&format!(" key={}", line_safe(key)));
            }
            if let Some(value) = &node.text {
                text.push_str(&format!(" \"{}\"", line_safe(value)));
            }
            if let Some(style) = &node.style {
                text.push_str(&format!(" style={}", line_safe(style)));
            }
            if node.selected {
                text.push_str(" <- selected");
            }
            lines.push(text);
        }
        pending.extend(
            node.children
                .iter()
                .rev()
                .map(|child| (child, level.saturating_add(1))),
        );
    }
    let footer = format!("│ nodes {count} of {node_limit}   depth {depth} of {depth_limit}");
    if rows == 1 {
        lines = vec![footer];
    } else {
        lines.truncate(rows - 1);
        lines.push(footer);
        if lines.len() < rows {
            lines.push("╰".into());
        }
    }
    lines
        .into_iter()
        .map(|line| line.chars().take(columns).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
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
pub fn render_memory_view(plugin: &str, view: &MemoryView) -> String {
    let mut lines = vec![
        format!("╭ Memory: {plugin} ({}) ", view.side.as_str()),
        format!(
            "linear memory {} of {}",
            format_binary(view.linear_bytes),
            format_binary(view.limit_bytes)
        ),
        format!("peak {}", format_binary(view.peak_bytes)),
        format!("storage {} of 1 MiB", format_binary(view.storage_bytes)),
        format!("instances {}   restarts {}", view.instances, view.restarts),
    ];
    if view.limit_bytes > 0 && u128::from(view.peak_bytes) * 5 > u128::from(view.limit_bytes) * 4 {
        lines.push("warning: peak above 80 percent of memory_mib".into());
    }
    lines.join("\n")
}

/// Bytes as a binary-prefixed figure such as `9.8 MiB` (DEV-686).
pub fn format_binary(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = "KiB";
    for name in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        value /= 1024.0;
        unit = name;
        if value < 1024.0 {
            break;
        }
    }
    if value >= 10.0 || value.fract() == 0.0 {
        format!("{value:.0} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
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

/// One explicit profiling period (DEV-690, DEV-691), separate from the fixed-size
/// side counters. Retains actual duration samples for exact period percentiles;
/// the host drops or clears it when the requested profiling period ends.
#[derive(Debug, Default)]
pub struct Profiler {
    samples: BTreeMap<String, Vec<Duration>>,
    overhead_ns: u128,
    measured_ns: u128,
}

impl Profiler {
    /// An empty profiler.
    pub fn new() -> Profiler {
        Self::default()
    }

    /// Records one call of `export`.
    pub fn record(&mut self, export: &str, took: Duration) {
        let start = std::time::Instant::now();
        self.samples.entry(export.into()).or_default().push(took);
        self.measured_ns = self.measured_ns.saturating_add(took.as_nanos());
        self.overhead_ns = self.overhead_ns.saturating_add(start.elapsed().as_nanos());
    }

    /// Ends the profiling period and releases all duration samples.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.overhead_ns = 0;
        self.measured_ns = 0;
    }
    /// The timing summary of `export`.
    pub fn timing(&self, export: &str) -> TimingRecord {
        let mut samples = self.samples.get(export).cloned().unwrap_or_default();
        samples.sort_unstable();
        let calls = u64::try_from(samples.len()).unwrap_or(u64::MAX);
        let budget = match export {
            "on-events" => Some(Duration::from_millis(20)),
            "render" => Some(Duration::from_millis(10)),
            _ => None,
        };
        let percentile = |fraction: f64| {
            if samples.is_empty() {
                return None;
            }
            let rank = fraction * (samples.len() - 1) as f64;
            let low = rank.floor() as usize;
            let high = rank.ceil() as usize;
            Some(
                (samples[low].as_secs_f64()
                    + (samples[high].as_secs_f64() - samples[low].as_secs_f64()) * rank.fract())
                    * 1000.0,
            )
        };
        TimingRecord {
            calls,
            total_ms: samples
                .iter()
                .map(|sample| sample.as_secs_f64() * 1000.0)
                .sum(),
            p50_ms: percentile(0.5),
            p95_ms: percentile(0.95),
            max_ms: samples.last().map(|sample| sample.as_secs_f64() * 1000.0),
            over_budget: u64::try_from(
                samples
                    .iter()
                    .filter(|sample| budget.is_some_and(|budget| **sample > budget))
                    .count(),
            )
            .unwrap_or(u64::MAX),
        }
    }
    /// Overrides baseline export budgets with the actual resolved call budget.
    pub fn timing_with_budget(&self, export: &str, budget: Duration) -> TimingRecord {
        let mut timing = self.timing(export);
        timing.over_budget = self.samples.get(export).map_or(0, |samples| {
            u64::try_from(samples.iter().filter(|sample| **sample > budget).count())
                .unwrap_or(u64::MAX)
        });
        timing
    }

    /// The profiler's own overhead as a fraction of the measured time (DEV-691).
    pub fn overhead_fraction(&self) -> f64 {
        if self.measured_ns == 0 {
            if self.overhead_ns == 0 {
                0.0
            } else {
                f64::INFINITY
            }
        } else {
            self.overhead_ns as f64 / self.measured_ns as f64
        }
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
    /// Actual release-removal/disable transaction before a replacement dev install.
    /// The default refuses: an admission flag alone cannot replace a running release.
    fn replace_release(&self, _id: &str) -> Result<(), Refusal> {
        Err(log_refusal(
            "dev.context-required",
            "release replacement requires an installer backend",
        ))
    }
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
    backend: std::sync::Arc<dyn DevBackend>,
    roots: Option<DevDataRoots>,
    plugin: Option<String>,
    running_version: Option<String>,
}

/// Actual host-selected data roots. A dev install never guesses a profile or project location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevDataRoots {
    /// Parent folder for development instances.
    pub dev: PathBuf,
    /// Parent folder for installed releases, used only for isolation validation.
    pub release: PathBuf,
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
    /// A backend without location context. `start` refuses until actual roots are supplied.
    pub fn new(backend: std::sync::Arc<dyn DevBackend>) -> Self {
        Self {
            backend,
            roots: None,
            plugin: None,
            running_version: None,
        }
    }

    /// Uses the caller's actual dev/release data roots; no writes happen in the constructor.
    pub fn with_data_roots(backend: std::sync::Arc<dyn DevBackend>, roots: DevDataRoots) -> Self {
        Self {
            roots: Some(roots),
            ..Self::new(backend)
        }
    }

    /// Admits one development install, creates its private data directory and returns its UI labels.
    /// Actual host registration must consume this result; these labels do not fabricate registration.
    pub fn start(&mut self, id: &str, replace_installed: bool) -> Result<DevInstall, Refusal> {
        check_plugin_log_id(id)
            .map_err(|_| log_refusal("dev.id-invalid", "invalid development plugin id"))?;
        if self.plugin.is_some() {
            return Err(log_refusal(
                "dev.already-started",
                "this development loop already owns a plugin",
            ));
        }
        let release_installed = self.backend.release_installed(id);
        if release_installed && !replace_installed {
            return Err(log_refusal(
                "dev.id-installed",
                "a release of this plugin is already installed",
            ));
        }
        let roots = self.roots.as_ref().ok_or_else(|| {
            log_refusal(
                "dev.context-required",
                "actual development and release data roots are required",
            )
        })?;
        if !roots.dev.is_absolute() || !roots.release.is_absolute() {
            return Err(log_refusal(
                "dev.data-path",
                "development and release data roots must be absolute",
            ));
        }
        check_directory_ancestry(&roots.dev)?;
        check_directory_ancestry(&roots.release)?;
        private_log_directory(&roots.dev)?;
        // A missing release root is still validated lexically; an existing root is resolved
        // only after rejecting symlink ancestry. The host must supply its real release root.
        let release = match std::fs::canonicalize(&roots.release) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if roots
                    .release
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    return Err(log_refusal(
                        "dev.data-path",
                        "release root contains parent traversal",
                    ));
                }
                roots.release.clone()
            }
            Err(error) => return Err(log_io(error)),
        };
        let dev = std::fs::canonicalize(&roots.dev).map_err(log_io)?;
        if dev.starts_with(&release) || release.starts_with(&dev) {
            return Err(log_refusal(
                "dev.data-path",
                "development and release data must use disjoint roots",
            ));
        }
        let data_dir = dev.join(id);
        let release_data_dir = release.join(id);
        private_log_directory(&data_dir)?;
        if release_installed {
            self.backend.replace_release(id)?;
        }
        self.plugin = Some(id.into());
        Ok(DevInstall {
            badge: "[dev]".into(),
            list_label: format!("{id} [dev]"),
            settings_label: format!("{id} [dev]"),
            data_dir,
            release_data_dir,
        })
    }

    /// Builds before touching the running instance. A failed build leaves it and its data intact.
    /// Backend methods are the actual runtime seam; this coordinator never simulates delivery.
    pub fn rebuild(&mut self) -> DevReport {
        if self.plugin.is_none() {
            return DevReport {
                reloaded: false,
                storage_kept: true,
                running_version: self.running_version.clone().unwrap_or_default(),
                errors: vec!["dev.not-started: start an admitted development install first".into()],
            };
        }
        match self.backend.build() {
            RebuildOutcome::Failed { errors } => DevReport {
                reloaded: false,
                storage_kept: true,
                running_version: self.running_version.clone().unwrap_or_default(),
                errors,
            },
            RebuildOutcome::Built { version } => {
                if version.is_empty() {
                    return DevReport {
                        reloaded: false,
                        storage_kept: true,
                        running_version: self.running_version.clone().unwrap_or_default(),
                        errors: vec!["dev.build-invalid: backend supplied no built version".into()],
                    };
                }
                self.backend.deactivate("upgrade");
                self.backend.load(&version);
                self.backend.activate();
                self.running_version = Some(version.clone());
                DevReport {
                    reloaded: true,
                    storage_kept: true,
                    running_version: version,
                    errors: Vec::new(),
                }
            }
        }
    }
}
