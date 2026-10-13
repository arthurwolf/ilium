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
    unimplemented!("api::abi::check_submission_count")
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
    unimplemented!("api::abi::batch_verdicts")
}

/// ABI-011: whether a correlation number belongs to the SDK.
pub fn is_sdk_correlation(correlation: u64) -> bool {
    unimplemented!("api::abi::is_sdk_correlation")
}

/// ABI-042: the instance deadline in ticks for a call deadline of `deadline_ms`:
/// `ceil(deadline_ms / 10) + 1`.
pub fn deadline_ticks(deadline_ms: u64) -> u64 {
    unimplemented!("api::abi::deadline_ticks")
}

/// ABI-045: the batch sizes that deliver `queued` events, at most 256 each, in order.
pub fn batch_sizes(queued: usize) -> Vec<usize> {
    unimplemented!("api::abi::batch_sizes")
}

/// ABI-044: whether `deactivate` bytes of this length are kept for the next activation.
pub fn restore_bytes_accepted(length: usize) -> bool {
    unimplemented!("api::abi::restore_bytes_accepted")
}

/// ABI-048: whether an export (as named in [`PURE_EXPORTS`]) is a pure call.
pub fn is_pure_export(export: &str) -> bool {
    unimplemented!("api::abi::is_pure_export")
}

/// ABI-048: the error an effectful import returns inside a pure call.
pub fn effectful_import_error() -> PluginError {
    unimplemented!("api::abi::effectful_import_error")
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
    unimplemented!("api::abi::delivery_policy")
}

/// ABI-015, ABI-039: whether losing or merging this event case is reported in `events-dropped`.
pub fn counted_in_events_dropped(event_case: &str) -> bool {
    unimplemented!("api::abi::counted_in_events_dropped")
}

/// A pure model of one plugin side's event queue, for the queue rules (ABI-033, ABI-036,
/// ABI-039, ABI-045). The host queue must behave like it.
#[derive(Debug, Clone)]
pub struct QueueModel {
    // Read by the implementation that replaces the stubs.
    #[allow(dead_code)]
    capacity: usize,
}

impl QueueModel {
    /// An empty queue holding at most `capacity` events (ABI-045).
    pub fn new(capacity: usize) -> QueueModel {
        QueueModel { capacity }
    }

    /// Enqueues one event of `event_case` with coalescing `key` (ABI-033, ABI-039).
    pub fn push(&mut self, event_case: &str, key: u64) {
        unimplemented!("api::abi::QueueModel::push")
    }

    /// Events queued now, `events-dropped` included (ABI-033).
    pub fn len(&self) -> usize {
        unimplemented!("api::abi::QueueModel::len")
    }

    /// Whether nothing is queued (ABI-033).
    pub fn is_empty(&self) -> bool {
        unimplemented!("api::abi::QueueModel::is_empty")
    }

    /// Total events evicted and counted (ABI-033).
    pub fn dropped_total(&self) -> u64 {
        unimplemented!("api::abi::QueueModel::dropped_total")
    }

    /// The pending `events-dropped` records as (event case, count), one per (event, source) (ABI-036).
    pub fn events_dropped_records(&self) -> Vec<(String, u64)> {
        unimplemented!("api::abi::QueueModel::events_dropped_records")
    }
}
