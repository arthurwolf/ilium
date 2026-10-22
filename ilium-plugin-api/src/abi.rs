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
    if count <= SUBMISSION_BATCH_LIMIT { Ok(()) } else { Err(Code::from_static("trap.submission-batch")) }
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
    let offender = checks.iter().position(|check| !check.shape_ok).map(|index| (index, ErrorCode::InvalidArgument))
        .or_else(|| checks.iter().position(|check| !check.grant_ok).map(|index| (index, ErrorCode::NotPermitted)));
    checks.iter().enumerate().map(|(index, _)| match offender {
        None => BatchVerdict::Proceed,
        Some((bad, code)) if index == bad => BatchVerdict::Refused(code),
        Some((bad, _)) => BatchVerdict::Cancelled(format!("{BATCH_REFUSED_PREFIX}{bad}")),
    }).collect()
}

/// ABI-011: whether a correlation number belongs to the SDK.
pub fn is_sdk_correlation(correlation: u64) -> bool { correlation >= SDK_CORRELATION_FLOOR }

/// ABI-042: the instance deadline in ticks for a call deadline of `deadline_ms`:
/// `ceil(deadline_ms / 10) + 1`.
pub fn deadline_ticks(deadline_ms: u64) -> u64 { deadline_ms.div_ceil(EPOCH_TICK_MS) + 1 }

/// ABI-045: the batch sizes that deliver `queued` events, at most 256 each, in order.
pub fn batch_sizes(queued: usize) -> Vec<usize> {
    let mut batches = vec![MAX_EVENTS_PER_BATCH; queued / MAX_EVENTS_PER_BATCH];
    if !queued.is_multiple_of(MAX_EVENTS_PER_BATCH) { batches.push(queued % MAX_EVENTS_PER_BATCH); }
    batches
}

/// ABI-044: whether `deactivate` bytes of this length are kept for the next activation.
pub fn restore_bytes_accepted(length: usize) -> bool { length <= RESTORE_BYTES_MAX }

/// ABI-048: whether an export (as named in [`PURE_EXPORTS`]) is a pure call.
pub fn is_pure_export(export: &str) -> bool { PURE_EXPORTS.contains(&export) }

/// ABI-048: the error an effectful import returns inside a pure call.
pub fn effectful_import_error() -> PluginError {
    PluginError { code: ErrorCode::NotPermitted, message: EFFECTFUL_IMPORT_MESSAGE.into(), retry_after_ms: None }
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
pub fn delivery_policy(_event_case: &str) -> Option<DeliveryPolicy> {
    unimplemented!("api::abi::delivery_policy")
}

/// ABI-015, ABI-039: whether losing or merging this event case is reported in `events-dropped`.
pub fn counted_in_events_dropped(_event_case: &str) -> bool {
    unimplemented!("api::abi::counted_in_events_dropped")
}

/// A pure model of one plugin side's event queue, for the queue rules (ABI-033, ABI-036,
/// ABI-039, ABI-045). The host queue must behave like it.
#[derive(Debug, Clone)]
pub struct QueueModel {
    capacity: usize,
}

impl QueueModel {
    /// An empty queue holding at most `capacity` events (ABI-045).
    pub fn new(capacity: usize) -> QueueModel {
        QueueModel { capacity }
    }

    /// The most events this queue holds before it starts dropping (ABI-045).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Enqueues one event of `event_case` with coalescing `key` (ABI-033, ABI-039).
    pub fn push(&mut self, _event_case: &str, _key: u64) {
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
        let valid = BatchCheck { shape_ok: true, grant_ok: true };
        assert_eq!(
            batch_verdicts(&[valid, valid, valid]),
            vec![BatchVerdict::Proceed, BatchVerdict::Proceed, BatchVerdict::Proceed]
        );
    }

    /// REQ: ABI-031
    #[test]
    fn batch_verdicts_shape_is_checked_over_whole_list_before_grant() {
        // Index 0 lacks its grant, index 2 is malformed: the shape check decides first,
        // so index 2 is the offender and index 0 is cancelled with its message.
        let checks = [
            BatchCheck { shape_ok: true, grant_ok: false },
            BatchCheck { shape_ok: true, grant_ok: true },
            BatchCheck { shape_ok: false, grant_ok: true },
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
            BatchCheck { shape_ok: true, grant_ok: true },
            BatchCheck { shape_ok: true, grant_ok: false },
            BatchCheck { shape_ok: true, grant_ok: false },
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
            BatchCheck { shape_ok: false, grant_ok: true },
            BatchCheck { shape_ok: true, grant_ok: true },
            BatchCheck { shape_ok: false, grant_ok: true },
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
        let checks = [BatchCheck { shape_ok: false, grant_ok: false }];
        assert_eq!(batch_verdicts(&checks), vec![BatchVerdict::Refused(ErrorCode::InvalidArgument)]);
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
        for case in ["screen-changed", "pane-focused", "tree-changed", "events-dropped"] {
            assert_eq!(delivery_policy(case), Some(DeliveryPolicy::Coalesced), "{case}");
        }
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_drop_oldest_cases() {
        for case in ["output-matched", "topic-published", "audio-frames"] {
            assert_eq!(delivery_policy(case), Some(DeliveryPolicy::DropOldestCounted), "{case}");
        }
    }

    /// REQ: ABI-015
    #[test]
    fn delivery_policy_never_dropped_cases() {
        for case in ["agent-activity-changed", "message", "pane-exited", "request-completed"] {
            assert_eq!(delivery_policy(case), Some(DeliveryPolicy::NeverDropped), "{case}");
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
        for case in ["output-matched", "topic-published", "output-matched", "topic-published", "output-matched"] {
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
