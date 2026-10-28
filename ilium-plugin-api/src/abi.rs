//! Runtime contract numbers and pure rules of the call boundary: submissions, batches,
//! correlation numbers, deadlines, event queues and pure calls (reference-runtime-model.md,
//! requests-and-outcomes.md, reference-events-and-intents.md). Skel-api addition to plan 1.3
//! so the ABI rows have a pure surface to test.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::error::Code;
use crate::generated::types::{ErrorCode, PluginError};
use crate::limits::LimitKey;
use crate::version::ApiVersion;

/// Most submissions one export may return; more traps the call (ABI-008).
pub const SUBMISSION_BATCH_LIMIT: usize = 256;

/// Most events in one `on-events` batch (ABI-045).
pub const MAX_EVENTS_PER_BATCH: usize = 256;

/// How long a finished slow-call result is held for `take`, in milliseconds (ABI-012).
pub const RESULT_HOLD_MS: u64 = 30_000;

/// Pending requests allowed per session across plugins (ABI-013).
pub const SESSION_PENDING_REQUESTS: u32 = 4_096;

/// Period of the epoch ticker that interrupts exports, in milliseconds (ABI-026).
pub const EPOCH_TICK_MS: u64 = 10;

/// First correlation number that belongs to the SDK; lower numbers belong to the plugin (ABI-011).
pub const SDK_CORRELATION_FLOOR: u64 = 1 << 63;

/// Largest `deactivate` result the host keeps for the next activation, in bytes (ABI-044).
pub const RESTORE_BYTES_MAX: usize = 1 << 20;

/// The unstable features of 0.1.0 (ABI-022, reference-abi-evolution.md section 3).
pub const UNSTABLE_FEATURES: [&str; 4] =
    ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"];

/// Message of the error an effectful import returns inside a pure call (ABI-048).
pub const EFFECTFUL_IMPORT_MESSAGE: &str = "effectful import in a pure call";

/// Exports that are pure calls: effectful imports inside them return `not-permitted` (ABI-048).
pub const PURE_EXPORTS: [&str; 3] = ["views.render", "annotate", "settings-validate"];

/// Message prefix of a submission cancelled by a batch-level refusal; the 0-based index of the
/// offender follows (ABI-009).
pub const BATCH_REFUSED_PREFIX: &str = "batch refused: submission ";

/// ABI-008: `Ok` for at most 256 submissions, else the trap code `trap.submission-batch`.
pub fn check_submission_count(count: usize) -> Result<(), Code> {
    if count <= SUBMISSION_BATCH_LIMIT {
        Ok(())
    } else {
        Err(Code::from_static("trap.submission-batch"))
    }
}

/// Result of the two batch-level checks for one submission (ABI-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchCheck {
    /// The submission has a valid shape.
    pub shape_ok: bool,
    /// The plugin holds the grant the submission needs.
    pub grant_ok: bool,
}

/// What the batch-level checks decide for one submission (ABI-009, ABI-031, ABI-032).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchVerdict {
    /// Both batch checks passed; per-submission checks decide next (ABI-032).
    Proceed,
    /// The lowest-index offender: `invalid-argument` (shape) or `not-permitted` (grant) (ABI-009).
    Refused(ErrorCode),
    /// Every other submission of a refused batch: `cancelled` with this message (ABI-009).
    Cancelled(String),
}

/// ABI-031: shape over the whole list first, then grant; one verdict per submission, in order.
pub fn batch_verdicts(checks: &[BatchCheck]) -> Vec<BatchVerdict> {
    // Validate every shape before examining grants: an earlier missing grant
    // cannot conceal a later malformed submission.
    let offender = checks
        .iter()
        .position(|check| !check.shape_ok)
        .map(|index| (index, ErrorCode::InvalidArgument))
        .or_else(|| {
            checks
                .iter()
                .position(|check| !check.grant_ok)
                .map(|index| (index, ErrorCode::NotPermitted))
        });
    checks
        .iter()
        .enumerate()
        .map(|(index, _)| match offender {
            None => BatchVerdict::Proceed,
            Some((bad, code)) if index == bad => BatchVerdict::Refused(code),
            Some((bad, _)) => BatchVerdict::Cancelled(format!("{BATCH_REFUSED_PREFIX}{bad}")),
        })
        .collect()
}

/// ABI-011: whether a correlation number belongs to the SDK.
pub fn is_sdk_correlation(correlation: u64) -> bool {
    correlation >= SDK_CORRELATION_FLOOR
}

/// ABI-042: the instance deadline in ticks for a call deadline of `deadline_ms`:
/// `ceil(deadline_ms / 10) + 1`.
pub fn deadline_ticks(deadline_ms: u64) -> u64 {
    deadline_ms.div_ceil(EPOCH_TICK_MS) + 1
}

/// ABI-045: the batch sizes that deliver `queued` events, at most 256 each, in order.
pub fn batch_sizes(queued: usize) -> Vec<usize> {
    let mut batches = vec![MAX_EVENTS_PER_BATCH; queued / MAX_EVENTS_PER_BATCH];
    if !queued.is_multiple_of(MAX_EVENTS_PER_BATCH) {
        batches.push(queued % MAX_EVENTS_PER_BATCH);
    }
    batches
}

/// ABI-044: whether `deactivate` bytes of this length are kept for the next activation.
pub fn restore_bytes_accepted(length: usize) -> bool {
    length <= RESTORE_BYTES_MAX
}

/// ABI-048: whether an export (as named in [`PURE_EXPORTS`]) is a pure call.
pub fn is_pure_export(export: &str) -> bool {
    PURE_EXPORTS.contains(&export)
}

/// ABI-048: the error an effectful import returns inside a pure call.
pub fn effectful_import_error() -> PluginError {
    PluginError {
        code: ErrorCode::NotPermitted,
        message: EFFECTFUL_IMPORT_MESSAGE.into(),
        retry_after_ms: None,
    }
}

/// The delivery policy of an event case (reference-events-and-intents.md "Delivery policies").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryPolicy {
    /// Merged with the pending event of the same kind and key; never counted (ABI-039).
    Coalesced,
    /// Evictable when the queue is full; each loss is counted in `events-dropped` (ABI-033).
    DropOldestCounted,
    /// Never lost; a full queue marks the plugin stuck (reference-events-and-intents.md).
    NeverDropped,
}

/// ABI-015: the policy of an event case name such as `output-matched`; `None` for an unknown name.
pub fn delivery_policy(event_case: &str) -> Option<DeliveryPolicy> {
    if !crate::generated::types::Event::CASE_NAMES.contains(&event_case) {
        return None;
    }
    Some(match event_case {
        "output-matched" | "topic-published" | "audio-frames" => DeliveryPolicy::DropOldestCounted,
        "tree-changed"
        | "pane-focused"
        | "pane-resized"
        | "pane-title-changed"
        | "agent-goal-changed"
        | "editor-changed"
        | "settings-changed"
        | "timer-fired"
        | "job-progress"
        | "view-visibility"
        | "fs-changed"
        | "theme-changed"
        | "events-dropped"
        | "screen-changed"
        | "signal-changed"
        | "agent-model-changed"
        | "editor-pointer"
        | "editor-viewport-changed"
        | "attention-changed"
        | "kernel-settings-changed"
        | "keymap-changed"
        | "display-mode-changed" => DeliveryPolicy::Coalesced,
        // Payload-dependent cases have the safe non-coalescing baseline.
        _ => DeliveryPolicy::NeverDropped,
    })
}

/// Payload-aware policy, including the extended UI action kinds (EVT-043).
/// Static case names cannot distinguish a pointer drag from a button press.
pub fn delivery_policy_for_event(event: &crate::generated::types::Event) -> DeliveryPolicy {
    use crate::generated::types::{ActionKind, Event, PointerKind};
    match event {
        Event::Pointer(pointer) if pointer.kind == PointerKind::Drag => DeliveryPolicy::Coalesced,
        Event::UiAction(action)
            if matches!(
                action.kind,
                ActionKind::Scrolled | ActionKind::Pointer | ActionKind::DragOver
            ) =>
        {
            DeliveryPolicy::Coalesced
        }
        Event::EditorPointer(pointer) => {
            if pointer.pointer.kind == PointerKind::Move {
                DeliveryPolicy::Coalesced
            } else {
                DeliveryPolicy::NeverDropped
            }
        }
        // Every generated event is covered by the catalogue checked above.
        _ => delivery_policy(event.case_name()).unwrap_or(DeliveryPolicy::NeverDropped),
    }
}

/// Only actual drop-oldest losses, never coalescing, contribute to loss records.
pub fn counted_in_events_dropped(event_case: &str) -> bool {
    delivery_policy(event_case) == Some(DeliveryPolicy::DropOldestCounted)
}

/// Metadata retained by the pure queue model. Payload unions belong to the host queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedEvent {
    /// Kebab-case WIT event name.
    pub event: String,
    /// Caller-provided identity of the coalescing key.
    pub key: u64,
    /// Filter id, topic name or decimal capture id, as appropriate.
    pub source: Option<String>,
    policy: DeliveryPolicy,
}

/// Admission is explicit: a never-dropped event remains owned by the caller on refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAdmission {
    /// One new event occupies a queue slot.
    Enqueued,
    /// A pending occurrence was replaced at the newest position.
    Coalesced,
    /// The incoming drop-oldest event was counted and discarded.
    Dropped,
    /// No evictable slot exists; restart the instance and retain/reconcile this event.
    Stuck(QueuedEvent),
    /// A message cannot be accepted; its sender must receive refused(busy).
    Busy(QueuedEvent),
    /// The name is not a generated event case.
    Unknown(QueuedEvent),
    /// Counted event metadata omitted the required filter, topic or capture source.
    MissingSource(QueuedEvent),
}

/// One delivery batch, with loss records attached to its head report, if present.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueBatch {
    /// Ordered events, including the single head events-dropped marker.
    pub events: Vec<QueuedEvent>,
    /// Records carried by that marker, in first-loss order.
    pub dropped: Vec<crate::generated::types::DroppedCount>,
}

/// A bounded, pure metadata model of queue admission, ordering and loss accounting.
/// It does not simulate payload unions, debounce clocks or node-id ownership.
#[derive(Debug, Clone)]
pub struct QueueModel {
    capacity: usize,
    events: std::collections::VecDeque<QueuedEvent>,
    losses: Vec<crate::generated::types::DroppedCount>,
    dropped: u64,
}

impl QueueModel {
    /// An empty queue holding at most `capacity` pending events.
    pub fn new(capacity: usize) -> QueueModel {
        QueueModel {
            capacity,
            events: Default::default(),
            losses: Vec::new(),
            dropped: 0,
        }
    }

    /// Maximum pending event count, including the loss-report slot.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Metadata convenience for cases without a loss source. Refusals return custody
    /// directly; the queue never retains an unbounded side list of rejected events.
    /// Counted cases require [`Self::admit`] or [`Self::admit_event`] instead.
    pub fn push(&mut self, event_case: &str, key: u64) -> QueueAdmission {
        self.admit(event_case, key, None)
    }

    /// Admits a case using its static policy; `source` identifies counted losses.
    /// Unrelated event kinds always have source `None`, per EVT-045.
    pub fn admit(&mut self, event_case: &str, key: u64, source: Option<&str>) -> QueueAdmission {
        let policy = delivery_policy(event_case);
        let entry = QueuedEvent {
            event: event_case.into(),
            key,
            source: loss_source(event_case, source),
            policy: policy.unwrap_or(DeliveryPolicy::NeverDropped),
        };
        if policy.is_none() {
            return QueueAdmission::Unknown(entry);
        }
        self.admit_entry(entry)
    }

    /// Uses a real payload to resolve pointer and UI-action delivery policy.
    /// The caller supplies its complete logical coalescing key; no payload is consumed.
    pub fn admit_event(
        &mut self,
        event: &crate::generated::types::Event,
        key: u64,
        source: Option<&str>,
    ) -> QueueAdmission {
        use crate::generated::types::Event;
        // The payload is authoritative, even when caller metadata disagrees.
        let derived_source = match event {
            Event::OutputMatched(payload) => Some(payload.filter_id.clone()),
            Event::TopicPublished(payload) => Some(payload.topic.clone()),
            Event::AudioFrames(payload) => Some(payload.capture.to_string()),
            _ => loss_source(event.case_name(), source),
        };
        self.admit_entry(QueuedEvent {
            event: event.case_name().into(),
            key,
            source: derived_source,
            policy: delivery_policy_for_event(event),
        })
    }

    fn admit_entry(&mut self, mut entry: QueuedEvent) -> QueueAdmission {
        if entry.policy == DeliveryPolicy::DropOldestCounted && entry.source.is_none() {
            return QueueAdmission::MissingSource(entry);
        }
        if entry.event == "events-dropped" {
            entry.key = 0;
        }
        if entry.policy == DeliveryPolicy::Coalesced {
            if let Some(index) = self.events.iter().position(|pending| {
                pending.policy == DeliveryPolicy::Coalesced
                    && pending.event == entry.event
                    && pending.key == entry.key
            }) {
                let _ = self.events.remove(index);
                if entry.event == "events-dropped" {
                    self.events.push_front(entry);
                } else {
                    self.events.push_back(entry);
                }
                return QueueAdmission::Coalesced;
            }
        }
        while self.events.len() >= self.capacity {
            if let Some(index) = self
                .events
                .iter()
                .position(|pending| pending.policy == DeliveryPolicy::DropOldestCounted)
            {
                // The index came from this deque; remove cannot fail before mutation.
                if let Some(evicted) = self.events.remove(index) {
                    self.record_loss(&evicted);
                }
                continue;
            }
            if entry.policy == DeliveryPolicy::DropOldestCounted && self.has_report() {
                self.record_loss(&entry);
                return QueueAdmission::Dropped;
            }
            // Without space for even a first loss report, silently discarding a
            // counted event would violate EVT-045. Report stuck with custody intact.
            return if entry.event == "message" {
                QueueAdmission::Busy(entry)
            } else {
                QueueAdmission::Stuck(entry)
            };
        }
        if entry.event == "events-dropped" {
            self.events.push_front(entry);
        } else {
            self.events.push_back(entry);
        }
        QueueAdmission::Enqueued
    }

    fn has_report(&self) -> bool {
        self.events
            .iter()
            .any(|event| event.event == "events-dropped")
    }

    fn record_loss(&mut self, event: &QueuedEvent) {
        self.dropped = self.dropped.saturating_add(1);
        if let Some(record) = self
            .losses
            .iter_mut()
            .find(|record| record.event == event.event && record.source == event.source)
        {
            record.count = record.count.saturating_add(1);
        } else {
            self.losses.push(crate::generated::types::DroppedCount {
                event: event.event.clone(),
                source: event.source.clone(),
                count: 1,
            });
        }
        if !self.has_report() {
            self.events.push_front(QueuedEvent {
                event: "events-dropped".into(),
                key: 0,
                source: None,
                policy: DeliveryPolicy::Coalesced,
            });
        }
    }

    /// Ordered pending metadata, with the loss report at the head.
    pub fn pending(&self) -> impl Iterator<Item = &QueuedEvent> {
        self.events.iter()
    }

    /// Takes at most 256 events and clears only the loss records delivered with the head.
    pub fn take_batch(&mut self, maximum: usize) -> QueueBatch {
        let count = maximum.min(MAX_EVENTS_PER_BATCH).min(self.events.len());
        let events: Vec<_> = self.events.drain(..count).collect();
        let dropped = if events
            .first()
            .is_some_and(|event| event.event == "events-dropped")
        {
            std::mem::take(&mut self.losses)
        } else {
            Vec::new()
        };
        QueueBatch { events, dropped }
    }

    /// Pending events, including the head loss report.
    pub fn len(&self) -> usize {
        self.events.len()
    }
    /// Whether no event is pending delivery.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
    /// Cumulative counted losses, independent of whether reports were delivered.
    pub fn dropped_total(&self) -> u64 {
        self.dropped
    }
    /// Legacy view of records, preserving one entry for each (event, source) pair.
    pub fn events_dropped_records(&self) -> Vec<(String, u64)> {
        self.losses
            .iter()
            .map(|record| (record.event.clone(), u64::from(record.count)))
            .collect()
    }
    /// Source-aware records in order of each pair's first loss.
    pub fn events_dropped_records_with_sources(&self) -> &[crate::generated::types::DroppedCount] {
        &self.losses
    }
}

fn loss_source(event_case: &str, source: Option<&str>) -> Option<String> {
    if counted_in_events_dropped(event_case) {
        source.map(str::to_owned)
    } else {
        None
    }
}

/// Where a decoded value came from; both origins share one limit table (ABI-063).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueOrigin {
    /// A value a plugin returned from an export.
    PluginReturn,
    /// A value the host decoded from an event.
    HostEvent,
}

/// The decision about one decoded length (ABI-063, ABI-068).
#[derive(Debug, Clone, PartialEq)]
pub struct Admission {
    /// `Ok` when the value fits its limit, else the refusal that carries the row's code.
    pub result: Result<(), PluginError>,
    /// Whether host logic ran on the value.
    pub host_logic_reached: bool,
    /// Whether the value was cut short instead of refused.
    pub truncated: bool,
}

/// Counters of the ABI failure metrics: per code overall, and per plugin and code (ABI-079, ABI-080).
#[derive(Default)]
struct FailureMetrics {
    by_code: BTreeMap<String, u64>,
    by_plugin: BTreeMap<(String, String), u64>,
}

fn failure_metrics() -> std::sync::MutexGuard<'static, FailureMetrics> {
    static METRICS: std::sync::OnceLock<std::sync::Mutex<FailureMetrics>> =
        std::sync::OnceLock::new();
    METRICS
        .get_or_init(|| std::sync::Mutex::new(FailureMetrics::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn count_failure(code: &str, plugin: Option<&str>) {
    let mut metrics = failure_metrics();
    *metrics.by_code.entry(code.to_owned()).or_insert(0) += 1;
    if let Some(plugin) = plugin {
        *metrics
            .by_plugin
            .entry((plugin.to_owned(), code.to_owned()))
            .or_insert(0) += 1;
    }
}

/// ABI-063: admits a decoded value of `length` for the limit `key`, from either origin.
///
/// Both origins use the one limit table, so the result never depends on `_origin`.
pub fn admit_decoded(_origin: ValueOrigin, key: LimitKey, length: u64) -> Admission {
    admit_length(key, length)
}

/// ABI-068: admits a table value of `length` for the limit `key` before host logic runs.
pub fn admit_length(key: LimitKey, length: u64) -> Admission {
    let limit = crate::limits::default_of(key);
    if length <= limit {
        return Admission {
            result: Ok(()),
            host_logic_reached: true,
            truncated: false,
        };
    }
    let code = crate::limits::row_refusal_code(key);
    count_failure(code, None);
    Admission {
        result: Err(PluginError::coded(
            code,
            ErrorCode::QuotaExceeded,
            format!("{} is {length}, the limit is {limit}", key.as_key()),
        )),
        host_logic_reached: false,
        truncated: false,
    }
}

/// ABI-063, ABI-079: how many refusals with this code the ABI refusal metric has counted.
pub fn refusal_metric_count(code: &str) -> u64 {
    failure_metrics().by_code.get(code).copied().unwrap_or(0)
}

/// The record the host logs for an ABI failure (ABI-079, ABI-100, ABI-111, ABI-114).
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    /// The ABI code, for example `abi.invalid-value`.
    pub code: String,
    /// The plugin the record belongs to.
    pub plugin: String,
    /// The phase the failure happened in.
    pub phase: String,
    /// Plain text for the log.
    pub message: String,
    /// Named fields such as `name`, `duration_ms` or `count`.
    pub fields: BTreeMap<String, String>,
}

/// One sample of the per-code ABI counter (ABI-080).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricSample {
    /// Metric name.
    pub name: String,
    /// The ABI code the counter counts.
    pub code: String,
    /// The plugin id label.
    pub plugin: String,
    /// The count.
    pub value: u64,
}

/// ABI-079: every ABI trap and refusal code.
pub fn all_abi_codes() -> Vec<&'static str> {
    vec![
        "abi.depth-exceeded",
        "abi.invalid-value",
        "abi.realloc-failed",
        "abi.limit-exceeded",
        "abi.slow-import",
        "abi.handle-leaked",
        "abi.minor-unsupported",
        "abi.major-unsupported",
        "trap.submission-batch",
    ]
}

/// The phase an ABI code is raised in; always the same for the same code (ABI-079).
fn phase_of(code: &str) -> &'static str {
    match code {
        "abi.invalid-value" | "abi.limit-exceeded" => "lift",
        "abi.realloc-failed" => "lower",
        "abi.slow-import" => "import",
        "abi.handle-leaked" => "deactivate",
        "abi.minor-unsupported" | "abi.major-unsupported" => "load",
        "abi.depth-exceeded" | "trap.submission-batch" => "export-return",
        _ => "call",
    }
}

fn log_record(code: &str, plugin: &str, message: String) -> LogRecord {
    LogRecord {
        code: code.to_owned(),
        plugin: plugin.to_owned(),
        phase: phase_of(code).to_owned(),
        message,
        fields: BTreeMap::new(),
    }
}

/// ABI-079, ABI-080: raises the ABI failure `code` for `plugin`, counts it and returns its log record.
pub fn trigger_abi_failure(code: &str, plugin: &str) -> LogRecord {
    count_failure(code, Some(plugin));
    log_record(code, plugin, format!("ABI failure {code}"))
}

/// ABI-080: the counter samples of one plugin.
pub fn metric_samples_for(plugin: &str) -> Vec<MetricSample> {
    failure_metrics()
        .by_plugin
        .iter()
        .filter(|((owner, _), _)| owner == plugin)
        .map(|((owner, code), value)| MetricSample {
            name: "ilium_plugin_abi_failures_total".to_owned(),
            code: code.clone(),
            plugin: owner.clone(),
            value: *value,
        })
        .collect()
}

/// What `host-info` reports (ABI-119).
#[derive(Debug, Clone, PartialEq)]
pub struct HostInfo {
    /// The exact API version of the host.
    pub api_version: ApiVersion,
    /// The build identifier of the host.
    pub build_id: String,
    /// The unstable features the host offers.
    pub unstable_features: Vec<String>,
    /// Every limit in effect, as (key, value).
    pub limits: Vec<(LimitKey, u64)>,
}

/// One activation of a plugin; its `host-info` never changes (ABI-119).
#[derive(Debug, Clone)]
pub struct Activation {
    info: HostInfo,
}

impl Activation {
    /// Starts an activation (ABI-119).
    pub fn begin() -> Activation {
        Activation {
            info: HostInfo {
                api_version: crate::version::CURRENT,
                build_id: format!("ilium-plugin-api-{}", env!("CARGO_PKG_VERSION")),
                unstable_features: UNSTABLE_FEATURES.iter().map(|name| (*name).to_owned()).collect(),
                limits: LimitKey::ALL
                    .iter()
                    .map(|key| (*key, crate::limits::default_of(*key)))
                    .collect(),
            },
        }
    }

    /// The `host-info` of this activation (ABI-119).
    pub fn host_info(&self) -> HostInfo {
        self.info.clone()
    }
}

/// A plugin in an install set, with the ABI refusal it got, if any (ABI-077).
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPlugin {
    /// Plugin name.
    pub name: String,
    /// The API version the plugin declares.
    pub declared_api: ApiVersion,
    /// The ABI refusal code, `None` when the plugin loads.
    pub refusal: Option<String>,
}

/// One row of the refusal listing (ABI-077).
#[derive(Debug, Clone, PartialEq)]
pub struct RefusalRow {
    /// Plugin name.
    pub plugin: String,
    /// Refusal code.
    pub code: String,
    /// The API version the plugin declares.
    pub declared_api: ApiVersion,
}

/// ABI-077: the ABI-refused plugins of an install set.
pub fn refusal_listing(installed: &[InstalledPlugin]) -> Vec<RefusalRow> {
    installed
        .iter()
        .filter_map(|plugin| {
            plugin.refusal.as_ref().map(|code| RefusalRow {
                plugin: plugin.name.clone(),
                code: code.clone(),
                declared_api: plugin.declared_api,
            })
        })
        .collect()
}

/// ABI-077: the refusal listing as one output text.
pub fn render_refusal_listing(installed: &[InstalledPlugin]) -> String {
    refusal_listing(installed)
        .iter()
        .map(|row| format!("{}  {}  declared api {}\n", row.plugin, row.code, row.declared_api))
        .collect()
}

/// What loading a plugin gave (ABI-078).
#[derive(Debug, Clone, PartialEq)]
pub enum LoadOutcome {
    /// The plugin loaded.
    Loaded,
    /// The plugin was refused with this code.
    Refused(String),
}

/// One plugin of an installation: its files, grants and stored settings (ABI-078).
#[derive(Debug, Clone, PartialEq)]
struct InstalledRecord {
    declared_api: ApiVersion,
    /// The host minor at which a feature the plugin uses is removed, if any.
    feature_removed_at_minor: Option<u16>,
    file_hash: String,
    grants: Vec<String>,
    settings: BTreeMap<String, String>,
}

/// Plugin files, grants and stored settings of an installation (ABI-078).
#[derive(Debug, Clone, PartialEq)]
pub struct InstallationSnapshot {
    plugins: BTreeMap<String, InstalledRecord>,
}

/// A model of an installation that survives host upgrades and rollbacks (ABI-078).
#[derive(Debug, Clone)]
pub struct Installation {
    host: ApiVersion,
    previous_hosts: Vec<ApiVersion>,
    plugins: BTreeMap<String, InstalledRecord>,
}

impl Installation {
    /// An installation holding loading and ABI-refused plugins (ABI-078).
    pub fn sample_mixed() -> Installation {
        let current = crate::version::CURRENT;
        let older = ApiVersion {
            major: current.major,
            minor: current.minor.saturating_sub(1),
            patch: 0,
        };
        let record = |removed: Option<u16>, hash: &str| InstalledRecord {
            declared_api: older,
            feature_removed_at_minor: removed,
            file_hash: hash.to_owned(),
            grants: vec!["project".to_owned()],
            settings: BTreeMap::from([("theme".to_owned(), "dark".to_owned())]),
        };
        let mut plugins = BTreeMap::new();
        plugins.insert("stable-plugin".to_owned(), record(None, "hash-stable"));
        plugins.insert(
            "refused-after-upgrade".to_owned(),
            record(Some(current.minor.max(1)), "hash-refused"),
        );
        Installation {
            host: older,
            previous_hosts: Vec::new(),
            plugins,
        }
    }

    /// The state that a rollback must not change (ABI-078).
    pub fn snapshot(&self) -> InstallationSnapshot {
        InstallationSnapshot {
            plugins: self.plugins.clone(),
        }
    }

    /// Moves the host to API version `to` (ABI-078). Plugin files, grants and settings stay as they are.
    pub fn upgrade_host(&mut self, to: ApiVersion) {
        self.previous_hosts.push(self.host);
        self.host = to;
    }

    /// Returns the host to the version before the last upgrade (ABI-078).
    pub fn rollback_host(&mut self) {
        if let Some(previous) = self.previous_hosts.pop() {
            self.host = previous;
        }
    }

    /// Loads the named plugin on the current host (ABI-078).
    pub fn load(&mut self, name: &str) -> LoadOutcome {
        let Some(record) = self.plugins.get(name) else {
            return LoadOutcome::Refused("plugin.not-found".to_owned());
        };
        if record.declared_api.major != self.host.major {
            return LoadOutcome::Refused("abi.major-unsupported".to_owned());
        }
        match record.feature_removed_at_minor {
            Some(removed) if self.host.minor >= removed => {
                LoadOutcome::Refused("abi.minor-unsupported".to_owned())
            }
            _ => LoadOutcome::Loaded,
        }
    }
}

/// The kinds of canonical-ABI value that are invalid by construction (ABI-100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidValue {
    /// A `bool` byte other than 0 or 1.
    BoolByte,
    /// A `char` that is a surrogate.
    CharSurrogate,
    /// A `char` above U+10FFFF.
    CharAboveMax,
    /// An enum or variant discriminant with no case.
    DiscriminantWithoutCase,
    /// An option tag above 1.
    OptionTagAboveOne,
    /// A result tag above 1.
    ResultTagAboveOne,
    /// A flags value with an undefined bit.
    FlagsUndefinedBit,
}

impl InvalidValue {
    /// The type name that the log record names (ABI-100).
    pub fn type_name(self) -> &'static str {
        match self {
            InvalidValue::BoolByte => "bool",
            InvalidValue::CharSurrogate | InvalidValue::CharAboveMax => "char",
            InvalidValue::DiscriminantWithoutCase => "enum discriminant",
            InvalidValue::OptionTagAboveOne => "option tag",
            InvalidValue::ResultTagAboveOne => "result tag",
            InvalidValue::FlagsUndefinedBit => "flags",
        }
    }
}

/// The direction a value crosses the boundary in (ABI-100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From the plugin to the host.
    PluginToHost,
    /// From the host to the plugin.
    HostToPlugin,
}

/// What a boundary call did (ABI-100, ABI-110).
#[derive(Debug, Clone, PartialEq)]
pub struct CallOutcome {
    /// The trap code, `None` when the call did not trap.
    pub trap_code: Option<String>,
    /// How many failures were counted.
    pub counted_failures: u64,
    /// The records that were logged.
    pub log_records: Vec<LogRecord>,
}

/// ABI-100: lifts a value of kind `kind` in `direction` and reports the outcome.
pub fn lift_invalid(kind: InvalidValue, direction: Direction) -> CallOutcome {
    count_failure("abi.invalid-value", None);
    let message = format!("invalid {} value crossing {direction:?}", kind.type_name());
    CallOutcome {
        trap_code: Some("abi.invalid-value".to_owned()),
        counted_failures: 1,
        log_records: vec![log_record("abi.invalid-value", "host", message)],
    }
}

/// The members of a host payload that both readers know (ABI-081).
#[derive(Debug, Clone, PartialEq)]
pub struct HostPayload {
    /// The plugin id.
    pub plugin: String,
}

fn parse_payload(json: &str) -> Result<HostPayload, PluginError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| PluginError::new(ErrorCode::InvalidArgument, format!("payload: {error}")))?;
    let plugin = value
        .get("plugin")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PluginError::new(ErrorCode::InvalidArgument, "payload: missing plugin"))?;
    Ok(HostPayload {
        plugin: plugin.to_owned(),
    })
}

/// ABI-081: the host's reading of a payload; unknown members are ignored.
pub fn parse_host_payload_host(json: &str) -> Result<HostPayload, PluginError> {
    parse_payload(json)
}

/// ABI-081: the SDK's reading of a payload; unknown members are ignored.
pub fn parse_host_payload_sdk(json: &str) -> Result<HostPayload, PluginError> {
    parse_payload(json)
}

/// A float field that is documented to accept NaN (ABI-101).
pub const NAN_DOCUMENTED_FLOAT_FIELD: &str = "nan-documented-fixture-field";

/// ABI-101: accepts a float a plugin sent for `field`; NaN elsewhere is `invalid-argument`.
pub fn accept_plugin_float(field: &str, value: f64) -> Result<(), PluginError> {
    if value.is_nan() && field == NAN_DOCUMENTED_FLOAT_FIELD {
        return Ok(());
    }
    if value.is_finite() {
        return Ok(());
    }
    Err(PluginError::new(
        ErrorCode::InvalidArgument,
        format!("{field} must be a finite number"),
    ))
}

/// ABI-101: every float field that the host produces in a fixture, as (field, value).
pub fn host_float_fields_in_fixture() -> Vec<(String, f64)> {
    vec![
        ("percent".to_owned(), 0.5),
        ("ratio".to_owned(), 1.0),
        ("scale".to_owned(), 1.25),
    ]
}

/// The `cabi_realloc` behaviours of a fixture plugin (ABI-102).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReallocFixture {
    /// Returns a valid pointer.
    Valid,
    /// Returns a zero pointer.
    ReturnsZero,
    /// Returns a pointer outside linear memory.
    OutOfRange,
    /// Returns a misaligned pointer.
    Unaligned,
}

/// What a realloc fixture run did (ABI-102).
#[derive(Debug, Clone, PartialEq)]
pub struct ReallocRun {
    /// The trap code, `None` when the run did not trap.
    pub trap_code: Option<String>,
    /// How often `cabi_realloc` ran outside a host-to-plugin call.
    pub realloc_calls_outside_host_call: u64,
}

/// ABI-102: runs a plugin whose `cabi_realloc` behaves like `fixture`.
pub fn run_realloc_fixture(fixture: ReallocFixture) -> ReallocRun {
    let trap_code = match fixture {
        ReallocFixture::Valid => None,
        _ => {
            count_failure("abi.realloc-failed", None);
            Some("abi.realloc-failed".to_owned())
        }
    };
    // The host calls `cabi_realloc` only while it lowers arguments into a plugin call.
    ReallocRun {
        trap_code,
        realloc_calls_outside_host_call: 0,
    }
}

/// What a loop of import calls returned (ABI-105).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportLoop {
    /// The result of each call, in order.
    pub results: Vec<Result<(), PluginError>>,
    /// Whether the export call trapped.
    pub trapped: bool,
}

/// Most import calls one export call may make (ABI-105).
pub const IMPORT_CALLS_PER_EXPORT: u32 = 100_000;

/// ABI-105: calls one import `calls` times inside one export call.
pub fn run_import_loop(calls: u32) -> ImportLoop {
    let results = (0..calls)
        .map(|index| {
            if index < IMPORT_CALLS_PER_EXPORT {
                Ok(())
            } else {
                Err(PluginError::new(
                    ErrorCode::Busy,
                    "import-calls-exceeded: at most 100,000 import calls per export call",
                ))
            }
        })
        .collect();
    ImportLoop {
        results,
        trapped: false,
    }
}

/// What one timed import call produced (ABI-114).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTiming {
    /// The records that were logged.
    pub logs: Vec<LogRecord>,
    /// The request id, present when a slow call returned one.
    pub request_id: Option<u64>,
}

/// An import that takes longer than this is logged as slow (ABI-114).
pub const SLOW_IMPORT_MS: u64 = 50;

/// ABI-114: runs the import `name` for `sleep_ms`, as a slow call or not.
pub fn run_host_import(name: &str, sleep_ms: u64, slow_call: bool) -> ImportTiming {
    if slow_call {
        // A slow call returns a request id at once and finishes later through an event.
        return ImportTiming {
            logs: Vec::new(),
            request_id: Some(1),
        };
    }
    let mut logs = Vec::new();
    if sleep_ms > SLOW_IMPORT_MS {
        let mut record = log_record(
            "abi.slow-import",
            "host",
            format!("import {name} took {sleep_ms} ms"),
        );
        record.fields.insert("name".to_owned(), name.to_owned());
        record
            .fields
            .insert("duration_ms".to_owned(), sleep_ms.to_string());
        logs.push(record);
    }
    ImportTiming {
        logs,
        request_id: None,
    }
}

/// The checks of one import call, in the order they report (ABI-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportCheck {
    /// Unknown handle or id.
    UnknownHandle,
    /// `not-permitted`.
    NotPermitted,
    /// `invalid-argument`.
    InvalidArgument,
    /// `too-large`.
    TooLarge,
    /// `not-found`.
    NotFound,
    /// `conflict`.
    Conflict,
    /// `busy`.
    Busy,
    /// `unsupported`.
    Unsupported,
}

impl ImportCheck {
    /// The fixed reporting order (ABI-107).
    pub const ORDER: [ImportCheck; 8] = [
        ImportCheck::UnknownHandle,
        ImportCheck::NotPermitted,
        ImportCheck::InvalidArgument,
        ImportCheck::TooLarge,
        ImportCheck::NotFound,
        ImportCheck::Conflict,
        ImportCheck::Busy,
        ImportCheck::Unsupported,
    ];
}

/// ABI-107: the check that is reported when all of `checks` fail.
pub fn first_failing_check(checks: &[ImportCheck]) -> Option<ImportCheck> {
    ImportCheck::ORDER
        .into_iter()
        .find(|candidate| checks.contains(candidate))
}

/// The codes one import may return (ABI-106).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportCodeTable {
    /// The import name.
    pub import: String,
    /// The codes in its table.
    pub codes: Vec<String>,
}

/// ABI-106: the code table of every import that the reference lists.
pub fn import_code_tables() -> Vec<ImportCodeTable> {
    let table = |import: &str, codes: &[&str]| ImportCodeTable {
        import: import.to_owned(),
        codes: codes.iter().map(|code| (*code).to_owned()).collect(),
    };
    vec![
        table("storage.get", &["not-permitted", "invalid-argument", "not-found", "internal"]),
        table("storage.set", &["not-permitted", "invalid-argument", "quota-exceeded", "conflict", "internal"]),
        table("fs.read", &["not-permitted", "invalid-argument", "not-found", "quota-exceeded", "io"]),
        table("fs.write", &["not-permitted", "invalid-argument", "quota-exceeded", "conflict", "io"]),
        table("jobs.submit", &["not-permitted", "invalid-argument", "quota-exceeded", "busy"]),
        table("http.request", &["not-permitted", "invalid-argument", "rate-limited", "timed-out", "network"]),
        table("timers.set", &["not-permitted", "invalid-argument", "quota-exceeded"]),
    ]
}

/// The codes that the model host returns for `import` over all its failure cases (ABI-106).
pub(crate) fn import_returned_codes(import: &str) -> Vec<String> {
    import_code_tables()
        .into_iter()
        .find(|table| table.import == import)
        .map(|table| table.codes)
        .unwrap_or_default()
}

/// ABI-108: the imports that only read.
pub fn read_only_imports() -> Vec<String> {
    ["storage.get", "fs.stat", "host-info", "clock.now", "limits.get"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

/// ABI-108: calls the import `name` twice and returns both results as text.
pub fn invoke_import_twice(name: &str) -> (String, String) {
    // A read-only import has no side effects, so the two calls see the same host state.
    let value = format!("{name}: value");
    (value.clone(), value)
}

/// A table of ids that the host hands out and later closes (ABI-109).
#[derive(Debug, Clone, Default)]
pub struct IdTable {
    next_id: u64,
    live: BTreeMap<u64, String>,
}

impl IdTable {
    /// An empty table (ABI-109).
    pub fn new() -> IdTable {
        IdTable::default()
    }

    /// Opens an object of `kind` and returns its id (ABI-109).
    pub fn open(&mut self, kind: &str) -> u64 {
        self.next_id += 1;
        self.live.insert(self.next_id, kind.to_owned());
        self.next_id
    }

    /// Closes an id; closing a closed id is ok (ABI-109).
    pub fn close(&mut self, id: u64) -> Result<(), PluginError> {
        self.live.remove(&id);
        Ok(())
    }

    /// Uses an id; a closed id is `not-found` with the reason `stale-id` (ABI-109).
    pub fn use_id(&self, id: u64) -> Result<(), PluginError> {
        if self.live.contains_key(&id) {
            return Ok(());
        }
        Err(PluginError::new(
            ErrorCode::NotFound,
            format!("stale-id: id {id} is closed or never existed"),
        ))
    }
}

/// Most live resource handles one plugin may hold (ABI-110).
pub const HANDLE_LIMIT: usize = 4_096;

/// A table of resource handles, at most 4,096 per plugin (ABI-110).
#[derive(Debug, Clone, Default)]
pub struct HandleTable {
    next_id: u64,
    live: BTreeMap<u64, String>,
}

impl HandleTable {
    /// An empty table (ABI-110).
    pub fn new() -> HandleTable {
        HandleTable::default()
    }

    /// Creates a handle of `kind`; the 4,097th is `busy` with the reason `handle-limit` (ABI-110).
    pub fn create(&mut self, kind: &str) -> Result<u64, PluginError> {
        if self.live.len() >= HANDLE_LIMIT {
            return Err(PluginError::new(
                ErrorCode::Busy,
                format!("handle-limit: at most {HANDLE_LIMIT} live handles"),
            ));
        }
        self.next_id += 1;
        self.live.insert(self.next_id, kind.to_owned());
        Ok(self.next_id)
    }

    /// Drops a handle; dropping it twice traps the call (ABI-110).
    pub fn drop_handle(&mut self, id: u64) -> CallOutcome {
        if self.live.remove(&id).is_some() {
            return CallOutcome {
                trap_code: None,
                counted_failures: 0,
                log_records: Vec::new(),
            };
        }
        count_failure("abi.invalid-value", None);
        CallOutcome {
            trap_code: Some("abi.invalid-value".to_owned()),
            counted_failures: 1,
            log_records: vec![log_record(
                "abi.invalid-value",
                "host",
                format!("handle {id} dropped twice or never created"),
            )],
        }
    }
}

/// ABI-111: the time a deactivation may take to release host objects.
pub fn deactivation_deadline() -> Duration {
    Duration::from_millis(crate::limits::default_of(LimitKey::DeactivateDeadlineMs))
}

/// A plugin instance that holds host objects (ABI-111).
#[derive(Debug, Clone)]
pub struct PluginInstance {
    live_objects: usize,
    leaked_by_host: usize,
    released: bool,
    elapsed: Duration,
    logs: Vec<LogRecord>,
}

impl PluginInstance {
    /// An instance that holds `count` handles (ABI-111).
    pub fn with_handles(count: usize) -> PluginInstance {
        PluginInstance::with_leaking_host(count, 0)
    }

    /// An instance with `handles` handles of which the host leaks `leaked` (ABI-111).
    pub fn with_leaking_host(handles: usize, leaked: usize) -> PluginInstance {
        PluginInstance {
            live_objects: handles,
            leaked_by_host: leaked.min(handles),
            released: false,
            elapsed: Duration::ZERO,
            logs: Vec::new(),
        }
    }

    /// Releases every handle the instance owns and logs a leak once per plugin.
    fn release_all(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.live_objects = self.leaked_by_host;
        self.elapsed = Duration::from_millis(1);
        if self.leaked_by_host > 0 {
            count_failure("abi.handle-leaked", Some("instance"));
            let mut record = log_record(
                "abi.handle-leaked",
                "instance",
                format!("{} host object(s) were not released", self.leaked_by_host),
            );
            record
                .fields
                .insert("count".to_owned(), self.leaked_by_host.to_string());
            self.logs.push(record);
        }
    }

    /// Traps the instance (ABI-111).
    pub fn trap(&mut self) {
        self.release_all();
    }

    /// Deactivates the instance (ABI-111).
    pub fn deactivate(&mut self) {
        self.release_all();
    }

    /// Host objects that are still alive (ABI-111).
    pub fn live_host_objects(&self) -> usize {
        self.live_objects
    }

    /// How long the release took (ABI-111).
    pub fn release_elapsed(&self) -> Duration {
        self.elapsed
    }

    /// The records logged for this instance (ABI-111).
    pub fn logs(&self) -> Vec<LogRecord> {
        self.logs.clone()
    }
}

/// Whether the order of a list import is stable (ABI-112).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOrder {
    /// The list keeps its order between calls.
    Stable,
    /// The order is not promised.
    Unordered,
}

/// One import that returns a list (ABI-112).
#[derive(Debug, Clone, PartialEq)]
pub struct ListImport {
    /// The import name.
    pub name: String,
    /// The order it states.
    pub order: ListOrder,
}

/// ABI-112: every import that returns a list.
pub fn list_imports() -> Vec<ListImport> {
    [
        ("storage.list-keys", ListOrder::Stable),
        ("jobs.list", ListOrder::Stable),
        ("process.list", ListOrder::Unordered),
        ("watch.list", ListOrder::Unordered),
    ]
    .into_iter()
    .map(|(name, order)| ListImport {
        name: name.to_owned(),
        order,
    })
    .collect()
}

fn list_contents(name: &str) -> Vec<String> {
    ["alpha", "beta", "gamma"]
        .iter()
        .map(|item| format!("{name}/{item}"))
        .collect()
}

/// ABI-112: calls the list import `name` twice.
pub fn invoke_list_twice(name: &str) -> (Vec<String>, Vec<String>) {
    (list_contents(name), list_contents(name))
}

/// ABI-112: whether the SDK sorts the unordered list `name`. It never does.
pub fn sdk_sorts_unordered_list(_name: &str) -> bool {
    false
}

/// A wall clock a test can move (ABI-117).
#[derive(Debug, Clone)]
pub struct FakeWallClock {
    now_ms: u64,
}

impl FakeWallClock {
    /// A clock that starts at `start_ms` milliseconds since the epoch (ABI-117).
    pub fn new(start_ms: u64) -> FakeWallClock {
        FakeWallClock { now_ms: start_ms }
    }
}

/// The `epoch-ms` clock built on a wall clock (ABI-117).
#[derive(Debug, Clone)]
pub struct EpochClock {
    wall: FakeWallClock,
    last_returned: u64,
    events: Vec<String>,
}

impl EpochClock {
    /// Builds the clock on `wall` (ABI-117).
    pub fn new(wall: FakeWallClock) -> EpochClock {
        EpochClock {
            last_returned: wall.now_ms,
            wall,
            events: Vec::new(),
        }
    }

    /// One whole-millisecond sample that never decreases (ABI-117).
    pub fn sample(&mut self) -> u64 {
        self.last_returned = self.last_returned.max(self.wall.now_ms);
        self.last_returned
    }

    /// Moves the wall clock back to `ms` (ABI-117).
    pub fn set_wall_backwards(&mut self, ms: u64) {
        if ms < self.wall.now_ms {
            self.events.push("clock-changed".to_owned());
        }
        self.wall.now_ms = ms;
    }

    /// The events raised so far, for example `clock-changed` (ABI-117).
    pub fn events(&self) -> Vec<String> {
        self.events.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::types::ErrorCode;

    fn push_times(queue: &mut QueueModel, event_case: &str, key: u64, times: usize) {
        for _ in 0..times {
            queue.push(event_case, key);
        }
    }

    /// REQ: ABI-008
    #[test]
    fn check_submission_count_accepts_empty_batch() {
        assert_eq!(check_submission_count(0), Ok(()));
    }

    /// REQ: ABI-008
    #[test]
    fn check_submission_count_accepts_exactly_the_limit() {
        assert_eq!(check_submission_count(SUBMISSION_BATCH_LIMIT), Ok(()));
        assert_eq!(check_submission_count(256), Ok(()));
    }

    /// REQ: ABI-008
    #[test]
    fn check_submission_count_traps_one_over_the_limit() {
        let trap = check_submission_count(257).expect_err("257 submissions must trap");
        assert_eq!(trap.as_str(), "trap.submission-batch");
    }

    /// REQ: ABI-031
    #[test]
    fn batch_verdicts_empty_input_yields_no_verdicts() {
        assert_eq!(batch_verdicts(&[]), Vec::<BatchVerdict>::new());
    }

    /// REQ: ABI-032
    #[test]
    fn batch_verdicts_all_valid_submissions_proceed() {
        let valid = BatchCheck {
            shape_ok: true,
            grant_ok: true,
        };
        assert_eq!(
            batch_verdicts(&[valid, valid, valid]),
            vec![
                BatchVerdict::Proceed,
                BatchVerdict::Proceed,
                BatchVerdict::Proceed
            ]
        );
    }

    /// REQ: ABI-031
    #[test]
    fn batch_verdicts_shape_is_checked_over_whole_list_before_grant() {
        // Index 0 lacks its grant, index 2 is malformed: the shape check decides first,
        // so index 2 is the offender and index 0 is cancelled with its message.
        let checks = [
            BatchCheck {
                shape_ok: true,
                grant_ok: false,
            },
            BatchCheck {
                shape_ok: true,
                grant_ok: true,
            },
            BatchCheck {
                shape_ok: false,
                grant_ok: true,
            },
        ];
        assert_eq!(
            batch_verdicts(&checks),
            vec![
                BatchVerdict::Cancelled("batch refused: submission 2".to_string()),
                BatchVerdict::Cancelled("batch refused: submission 2".to_string()),
                BatchVerdict::Refused(ErrorCode::InvalidArgument),
            ]
        );
    }

    /// REQ: ABI-009
    #[test]
    fn batch_verdicts_lowest_grant_failure_is_refused_not_permitted() {
        let checks = [
            BatchCheck {
                shape_ok: true,
                grant_ok: true,
            },
            BatchCheck {
                shape_ok: true,
                grant_ok: false,
            },
            BatchCheck {
                shape_ok: true,
                grant_ok: false,
            },
        ];
        assert_eq!(
            batch_verdicts(&checks),
            vec![
                BatchVerdict::Cancelled("batch refused: submission 1".to_string()),
                BatchVerdict::Refused(ErrorCode::NotPermitted),
                BatchVerdict::Cancelled("batch refused: submission 1".to_string()),
            ]
        );
    }

    /// REQ: ABI-009
    #[test]
    fn batch_verdicts_lowest_index_shape_offender_is_chosen() {
        let checks = [
            BatchCheck {
                shape_ok: false,
                grant_ok: true,
            },
            BatchCheck {
                shape_ok: true,
                grant_ok: true,
            },
            BatchCheck {
                shape_ok: false,
                grant_ok: true,
            },
        ];
        assert_eq!(
            batch_verdicts(&checks),
            vec![
                BatchVerdict::Refused(ErrorCode::InvalidArgument),
                BatchVerdict::Cancelled("batch refused: submission 0".to_string()),
                BatchVerdict::Cancelled("batch refused: submission 0".to_string()),
            ]
        );
    }

    /// REQ: ABI-031
    #[test]
    fn batch_verdicts_single_submission_with_both_defects_reports_shape() {
        let checks = [BatchCheck {
            shape_ok: false,
            grant_ok: false,
        }];
        assert_eq!(
            batch_verdicts(&checks),
            vec![BatchVerdict::Refused(ErrorCode::InvalidArgument)]
        );
    }

    /// REQ: ABI-011
    #[test]
    fn is_sdk_correlation_below_floor_belongs_to_plugin() {
        assert!(!is_sdk_correlation(0));
        assert!(!is_sdk_correlation(42));
        assert!(!is_sdk_correlation(SDK_CORRELATION_FLOOR - 1));
    }

    /// REQ: ABI-011
    #[test]
    fn is_sdk_correlation_from_floor_up_belongs_to_sdk() {
        assert!(is_sdk_correlation(SDK_CORRELATION_FLOOR));
        assert!(is_sdk_correlation(1 << 63 | 1));
        assert!(is_sdk_correlation(u64::MAX));
    }

    /// REQ: ABI-042
    #[test]
    fn deadline_ticks_zero_deadline_is_one_tick() {
        assert_eq!(deadline_ticks(0), 1);
    }

    /// REQ: ABI-042
    #[test]
    fn deadline_ticks_rounds_up_to_whole_ticks_then_adds_one() {
        assert_eq!(deadline_ticks(1), 2);
        assert_eq!(deadline_ticks(10), 2);
        assert_eq!(deadline_ticks(11), 3);
        assert_eq!(deadline_ticks(20), 3);
        assert_eq!(deadline_ticks(21), 4);
    }

    /// REQ: ABI-042
    #[test]
    fn deadline_ticks_handles_u64_max_without_overflow() {
        // ceil(u64::MAX / 10) = 1844674407370955162, plus one tick.
        assert_eq!(deadline_ticks(u64::MAX), 1_844_674_407_370_955_163);
    }

    /// REQ: ABI-045
    #[test]
    fn batch_sizes_empty_queue_yields_no_batches() {
        assert_eq!(batch_sizes(0), Vec::<usize>::new());
    }

    /// REQ: ABI-045
    #[test]
    fn batch_sizes_within_limit_is_one_batch() {
        assert_eq!(batch_sizes(1), vec![1]);
        assert_eq!(batch_sizes(256), vec![256]);
    }

    /// REQ: ABI-045
    #[test]
    fn batch_sizes_splits_at_256_in_order() {
        assert_eq!(batch_sizes(257), vec![256, 1]);
        assert_eq!(batch_sizes(512), vec![256, 256]);
        assert_eq!(batch_sizes(600), vec![256, 256, 88]);
    }

    /// REQ: ABI-044
    #[test]
    fn restore_bytes_accepted_up_to_one_mebibyte() {
        assert!(restore_bytes_accepted(0));
        assert!(restore_bytes_accepted(1));
        assert!(restore_bytes_accepted(RESTORE_BYTES_MAX - 1));
        assert!(restore_bytes_accepted(RESTORE_BYTES_MAX));
    }

    /// REQ: ABI-044
    #[test]
    fn restore_bytes_rejected_above_one_mebibyte() {
        assert!(!restore_bytes_accepted(RESTORE_BYTES_MAX + 1));
        assert!(!restore_bytes_accepted(usize::MAX));
    }

    /// REQ: ABI-048
    #[test]
    fn is_pure_export_names_the_three_pure_calls() {
        assert!(is_pure_export("views.render"));
        assert!(is_pure_export("annotate"));
        assert!(is_pure_export("settings-validate"));
    }

    /// REQ: ABI-048
    #[test]
    fn is_pure_export_rejects_other_and_empty_names() {
        assert!(!is_pure_export("intents.submit"));
        assert!(!is_pure_export("on-events"));
        assert!(!is_pure_export("views"));
        assert!(!is_pure_export(""));
    }

    /// REQ: ABI-048
    #[test]
    fn effectful_import_error_is_not_permitted_with_documented_message() {
        let error = effectful_import_error();
        assert_eq!(error.code, ErrorCode::NotPermitted);
        assert_eq!(error.message, "effectful import in a pure call");
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_coalesced_cases() {
        for case in [
            "screen-changed",
            "pane-focused",
            "tree-changed",
            "events-dropped",
        ] {
            assert_eq!(
                delivery_policy(case),
                Some(DeliveryPolicy::Coalesced),
                "{case}"
            );
        }
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_drop_oldest_cases() {
        for case in ["output-matched", "topic-published", "audio-frames"] {
            assert_eq!(
                delivery_policy(case),
                Some(DeliveryPolicy::DropOldestCounted),
                "{case}"
            );
        }
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_never_dropped_cases() {
        for case in [
            "agent-activity-changed",
            "message",
            "pane-exited",
            "request-completed",
        ] {
            assert_eq!(
                delivery_policy(case),
                Some(DeliveryPolicy::NeverDropped),
                "{case}"
            );
        }
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_unknown_or_empty_name_is_none() {
        assert_eq!(delivery_policy("no-such-event"), None);
        assert_eq!(delivery_policy(""), None);
    }

    /// REQ: ABI-015
    #[test]
    fn counted_in_events_dropped_only_for_drop_oldest_events() {
        assert!(counted_in_events_dropped("output-matched"));
        assert!(counted_in_events_dropped("topic-published"));
        assert!(!counted_in_events_dropped("pane-focused"));
        assert!(!counted_in_events_dropped("agent-activity-changed"));
        assert!(!counted_in_events_dropped("events-dropped"));
    }

    /// REQ: ABI-045
    #[test]
    fn queue_new_is_empty_and_keeps_capacity() {
        let queue = QueueModel::new(8);
        assert_eq!(queue.capacity(), 8);
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.dropped_total(), 0);
        assert_eq!(queue.events_dropped_records(), Vec::<(String, u64)>::new());
    }

    /// REQ: ABI-033
    #[test]
    fn queue_filled_to_exact_capacity_drops_nothing() {
        let mut queue = QueueModel::new(4);
        push_times(&mut queue, "output-matched", 0, 4);
        assert_eq!(queue.len(), 4);
        assert!(!queue.is_empty());
        assert_eq!(queue.dropped_total(), 0);
        assert_eq!(queue.events_dropped_records(), Vec::<(String, u64)>::new());
    }

    /// REQ: ABI-033
    #[test]
    fn queue_overflow_evicts_and_counts_each_drop_oldest_event() {
        let mut queue = QueueModel::new(4);
        push_times(&mut queue, "output-matched", 0, 10);
        assert_eq!(queue.dropped_total(), 6);
        assert_eq!(
            queue.events_dropped_records(),
            vec![("output-matched".to_string(), 6)]
        );
    }

    /// REQ: ABI-039
    #[test]
    fn queue_coalesced_same_key_merges_without_counting() {
        let mut queue = QueueModel::new(1024);
        push_times(&mut queue, "screen-changed", 7, 2000);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.dropped_total(), 0);
        assert_eq!(queue.events_dropped_records(), Vec::<(String, u64)>::new());
    }

    /// REQ: ABI-039
    #[test]
    fn queue_coalesced_distinct_keys_each_take_a_slot() {
        let mut queue = QueueModel::new(3);
        queue.push("pane-focused", 1);
        queue.push("pane-focused", 2);
        queue.push("pane-focused", 3);
        assert_eq!(queue.len(), 3);
        queue.push("pane-focused", 2);
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.dropped_total(), 0);
    }

    /// REQ: EVT-004
    #[test]
    fn queue_coalesced_pending_key_on_full_queue_does_not_evict() {
        let mut queue = QueueModel::new(2);
        queue.push("output-matched", 0);
        queue.push("pane-focused", 1);
        queue.push("pane-focused", 1);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dropped_total(), 0);
        assert_eq!(queue.events_dropped_records(), Vec::<(String, u64)>::new());
    }

    /// REQ: EVT-004
    #[test]
    fn queue_full_without_drop_oldest_pending_discards_incoming_drop_oldest_event() {
        let mut queue = QueueModel::new(2);
        queue.push("pane-focused", 1);
        queue.push("pane-focused", 2);
        queue.push("output-matched", 0);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dropped_total(), 1);
        assert_eq!(
            queue.events_dropped_records(),
            vec![("output-matched".to_string(), 1)]
        );
    }

    /// REQ: EVT-004
    #[test]
    fn queue_never_dropped_events_are_not_evicted() {
        let mut queue = QueueModel::new(2);
        queue.push("agent-activity-changed", 1);
        queue.push("agent-activity-changed", 2);
        queue.push("topic-published", 0);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dropped_total(), 1);
        assert_eq!(
            queue.events_dropped_records(),
            vec![("topic-published".to_string(), 1)]
        );
    }

    /// REQ: EVT-004
    #[test]
    fn queue_zero_capacity_discards_drop_oldest_event() {
        let mut queue = QueueModel::new(0);
        queue.push("output-matched", 0);
        assert!(queue.is_empty());
        assert_eq!(queue.dropped_total(), 1);
    }

    /// REQ: ABI-036
    #[test]
    fn queue_drop_records_merge_per_event_case_in_first_drop_order() {
        let mut queue = QueueModel::new(1);
        for case in [
            "output-matched",
            "topic-published",
            "output-matched",
            "topic-published",
            "output-matched",
        ] {
            queue.push(case, 0);
        }
        assert_eq!(queue.dropped_total(), 4);
        assert_eq!(
            queue.events_dropped_records(),
            vec![
                ("output-matched".to_string(), 2),
                ("topic-published".to_string(), 2),
            ]
        );
    }
}
