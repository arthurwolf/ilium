//! Event queue rules: ABI-015, ABI-033, ABI-036, ABI-039. The policy names are pure data
//! checked against the generated cases; the policy and queue rules call stubs.

use ilium_plugin_api::abi::{
    counted_in_events_dropped, delivery_policy, DeliveryPolicy, QueueModel,
};
use ilium_plugin_api::generated::types::Event;
use proptest::prelude::*;

/// Drop-oldest events (reference-events-and-intents.md "Delivery policies").
const DROP_OLDEST: [&str; 3] = ["output-matched", "topic-published", "audio-frames"];

/// Never-dropped events (reference-events-and-intents.md "Delivery policies").
const NEVER_DROPPED: [&str; 29] = [
    "node-removed",
    "pane-created",
    "pane-exited",
    "agent-identified",
    "agent-activity-changed",
    "input-submitted",
    "link-activated",
    "editor-opened",
    "editor-closed",
    "editor-saved",
    "request-completed",
    "message",
    "command-invoked",
    "key-pressed",
    "key-released",
    "cli-invoked",
    "cli-cancelled",
    "http-called",
    "client-attached",
    "client-detached",
    "user-line-submitted",
    "http-stream-data",
    "http-stream-closed",
    "child-output",
    "child-exited",
    "pane-lifecycle-changed",
    "client-request",
    "durable-timer-fired",
    "input-uncertain",
];

/// Coalesced events (reference-events-and-intents.md "Delivery policies"). `events-dropped`
/// is coalesced and also never dropped (ABI-036).
const COALESCED: [&str; 22] = [
    "tree-changed",
    "pane-focused",
    "pane-resized",
    "pane-title-changed",
    "agent-goal-changed",
    "editor-changed",
    "settings-changed",
    "timer-fired",
    "job-progress",
    "view-visibility",
    "fs-changed",
    "theme-changed",
    "events-dropped",
    "screen-changed",
    "signal-changed",
    "agent-model-changed",
    "editor-pointer",
    "editor-viewport-changed",
    "attention-changed",
    "kernel-settings-changed",
    "keymap-changed",
    "display-mode-changed",
];

/// Cases whose policy depends on the payload: `pointer` (drag coalesced) and `ui-action`
/// (scrolled coalesced). Not asserted as one policy.
const MIXED: [&str; 2] = ["pointer", "ui-action"];

/// REQ: ABI-015
#[test]
fn doc_abi_015_policy_lists_cover_every_event_case_once() {
    let mut listed: Vec<&str> = DROP_OLDEST
        .iter()
        .chain(NEVER_DROPPED.iter())
        .chain(COALESCED.iter())
        .chain(MIXED.iter())
        .copied()
        .collect();
    listed.sort_unstable();
    let mut cases = Event::CASE_NAMES.to_vec();
    cases.sort_unstable();
    assert_eq!(listed, cases);
}

/// REQ: ABI-015
#[test]
fn abi_015_drop_oldest_events() {
    for case in DROP_OLDEST {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::DropOldestCounted),
            "{case}"
        );
    }
}

/// REQ: ABI-015
#[test]
fn abi_015_never_dropped_events() {
    for case in NEVER_DROPPED {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::NeverDropped),
            "{case}"
        );
    }
}

/// REQ: ABI-015
#[test]
fn abi_015_coalesced_events() {
    for case in COALESCED {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::Coalesced),
            "{case}"
        );
    }
}

/// REQ: ABI-015
#[test]
fn abi_015_every_event_case_has_a_policy() {
    for case in Event::CASE_NAMES {
        assert!(delivery_policy(case).is_some(), "{case}");
    }
}

/// REQ: ABI-015
#[test]
fn abi_015_unknown_case_has_no_policy() {
    assert_eq!(delivery_policy("teleported"), None);
}

/// REQ: ABI-015, ABI-039
#[test]
fn abi_015_only_drop_oldest_losses_are_counted() {
    for case in DROP_OLDEST {
        assert!(counted_in_events_dropped(case), "{case}");
    }
    for case in NEVER_DROPPED.iter().chain(COALESCED.iter()) {
        assert!(!counted_in_events_dropped(case), "{case}");
    }
}

/// REQ: ABI-033
#[test]
fn abi_033_1500_output_matched_into_1024_drops_477() {
    // The events-dropped record holds one slot, so 1,023 events stay: 1,500 - 1,023 = 477.
    let mut queue = QueueModel::new(1_024);
    for key in 0..1_500 {
        queue.admit("output-matched", key, Some("fixture-source"));
    }
    assert_eq!(queue.dropped_total(), 477);
    assert_eq!(queue.len(), 1_024);
    assert_eq!(
        queue.events_dropped_records(),
        vec![("output-matched".to_owned(), 477)]
    );
}

/// REQ: ABI-033
#[test]
fn abi_033_queue_below_capacity_drops_nothing() {
    let mut queue = QueueModel::new(1_024);
    for key in 0..1_024 {
        queue.admit("output-matched", key, Some("fixture-source"));
    }
    assert_eq!(queue.dropped_total(), 0);
    assert_eq!(queue.len(), 1_024);
    assert!(queue.events_dropped_records().is_empty());
}

/// REQ: ABI-033
#[test]
fn abi_033_new_queue_is_empty() {
    let queue = QueueModel::new(1_024);
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
}

/// REQ: ABI-036
#[test]
fn abi_036_two_kinds_dropped_twice_give_one_event_with_two_records() {
    // Capacity 4: two output-matched and two topic-published fill it; three audio-frames
    // evict the four oldest drop-oldest events, one slot going to events-dropped.
    let mut queue = QueueModel::new(4);
    queue.admit("output-matched", 1, Some("fixture-source"));
    queue.admit("output-matched", 1, Some("fixture-source"));
    queue.admit("topic-published", 1, Some("fixture-source"));
    queue.admit("topic-published", 1, Some("fixture-source"));
    for _ in 0..3 {
        queue.admit("audio-frames", 1, Some("fixture-source"));
    }
    let mut records = queue.events_dropped_records();
    records.sort();
    assert_eq!(
        records,
        vec![
            ("output-matched".to_owned(), 2),
            ("topic-published".to_owned(), 2),
        ]
    );
    assert_eq!(queue.dropped_total(), 4);
}

/// REQ: ABI-039
#[test]
fn abi_039_2000_screen_changed_merge_without_events_dropped() {
    let mut queue = QueueModel::new(1_024);
    for _ in 0..2_000 {
        queue.push("screen-changed", 7);
    }
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.dropped_total(), 0);
    assert!(queue.events_dropped_records().is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-033
    #[test]
    fn abi_033_drop_count_is_pushes_minus_capacity_minus_one(capacity in 2usize..256, pushes in 0usize..1_024) {
        let mut queue = QueueModel::new(capacity);
        for key in 0..pushes as u64 {
            queue.admit("output-matched", key, Some("fixture-source"));
        }
        let expected = if pushes > capacity { (pushes - (capacity - 1)) as u64 } else { 0 };
        prop_assert_eq!(queue.dropped_total(), expected);
        prop_assert!(queue.len() <= capacity);
    }

    // REQ: ABI-039
    #[test]
    fn abi_039_coalesced_keys_never_count(keys in proptest::collection::vec(0u64..8, 0..2_000)) {
        let mut queue = QueueModel::new(1_024);
        for key in &keys {
            queue.push("screen-changed", *key);
        }
        prop_assert_eq!(queue.dropped_total(), 0);
        let distinct: std::collections::BTreeSet<_> = keys.iter().collect();
        prop_assert_eq!(queue.len(), distinct.len());
    }
}
