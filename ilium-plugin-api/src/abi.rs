//! Runtime contract numbers and pure rules of the call boundary: submissions, batches,
//! correlation numbers, deadlines, event queues and pure calls (reference-runtime-model.md,
//! requests-and-outcomes.md, reference-events-and-intents.md). Skel-api addition to plan 1.3
//! so the ABI rows have a pure surface to test.

use crate::error::Code;
use crate::generated::types::{ErrorCode, PluginError};

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
