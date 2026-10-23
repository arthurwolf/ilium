//! Queue admission boundaries beyond the catalogue conformance lane.
use ilium_plugin_api::abi::{
    DeliveryPolicy, QueueAdmission, QueueModel, delivery_policy, delivery_policy_for_event,
};
use ilium_plugin_api::generated::types::{ActionKind, Event, UiAction, ViewTarget};

#[test]
fn mixed_event_cases_have_conservative_static_policies() {
    assert_eq!(
        delivery_policy("pointer"),
        Some(DeliveryPolicy::NeverDropped)
    );
    assert_eq!(
        delivery_policy("ui-action"),
        Some(DeliveryPolicy::NeverDropped)
    );
    assert_eq!(delivery_policy("unknown"), None);
}

#[test]
fn extended_ui_action_policy_uses_real_payload_kind() {
    for kind in [
        ActionKind::Scrolled,
        ActionKind::Pointer,
        ActionKind::DragOver,
        ActionKind::Activated,
        ActionKind::Dropped,
    ] {
        let event = Event::UiAction(UiAction {
            view: ViewTarget::Panel("test".into()),
            key: "row".into(),
            action_id: "test".into(),
            kind,
            value: None,
        });
        let expected = if matches!(
            kind,
            ActionKind::Scrolled | ActionKind::Pointer | ActionKind::DragOver
        ) {
            DeliveryPolicy::Coalesced
        } else {
            DeliveryPolicy::NeverDropped
        };
        assert_eq!(delivery_policy_for_event(&event), expected);
    }
}

#[test]
fn coalescing_moves_to_newest_position_without_evicting_at_capacity() {
    let mut queue = QueueModel::new(2);
    queue.push("screen-changed", 7);
    queue.push("node-removed", 8);
    assert_eq!(
        queue.admit("screen-changed", 7, None),
        QueueAdmission::Coalesced
    );
    let pending: Vec<_> = queue
        .pending()
        .map(|event| (event.event.as_str(), event.key))
        .collect();
    assert_eq!(pending, [("node-removed", 8), ("screen-changed", 7)]);
    assert_eq!(queue.dropped_total(), 0);
}

#[test]
fn oldest_evictable_is_selected_across_kinds_and_sources() {
    let mut queue = QueueModel::new(4);
    queue.admit("node-removed", 1, None);
    queue.admit("output-matched", 2, Some("filter-b"));
    queue.admit("topic-published", 3, Some("topic-a"));
    queue.admit("output-matched", 4, Some("filter-a"));
    queue.admit("pane-created", 5, None);
    let records = queue.events_dropped_records_with_sources();
    assert_eq!(records.len(), 2);
    assert_eq!(
        (
            records[0].event.as_str(),
            records[0].source.as_deref(),
            records[0].count
        ),
        ("output-matched", Some("filter-b"), 1)
    );
    assert_eq!(
        (
            records[1].event.as_str(),
            records[1].source.as_deref(),
            records[1].count
        ),
        ("topic-published", Some("topic-a"), 1)
    );
    assert_eq!(queue.pending().next().unwrap().event, "events-dropped");
    assert_eq!(queue.len(), 4);
}

#[test]
fn repeated_losses_group_by_event_and_source_in_first_loss_order() {
    let mut queue = QueueModel::new(3);
    for source in ["b", "a", "b", "a", "b", "a", "b"] {
        queue.admit("output-matched", 0, Some(source));
    }
    let records = queue.events_dropped_records_with_sources();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source.as_deref(), Some("b"));
    assert_eq!(records[1].source.as_deref(), Some("a"));
    assert_eq!(
        records
            .iter()
            .map(|record| u64::from(record.count))
            .sum::<u64>(),
        queue.dropped_total()
    );
    assert_eq!(
        queue
            .pending()
            .filter(|event| event.event == "events-dropped")
            .count(),
        1
    );
}

#[test]
fn no_evictable_event_returns_custody_and_message_busy() {
    let mut queue = QueueModel::new(1);
    queue.push("node-removed", 1);
    assert!(
        matches!(queue.admit("pane-exited", 2, None), QueueAdmission::Stuck(event) if event.key == 2)
    );
    assert!(
        matches!(queue.admit("message", 3, None), QueueAdmission::Busy(event) if event.key == 3)
    );
    assert!(
        matches!(queue.push("pane-created", 4), QueueAdmission::Stuck(event) if event.key == 4)
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.dropped_total(), 0);
}

#[test]
fn a_new_coalescing_key_cannot_replace_a_different_key() {
    let mut queue = QueueModel::new(1);
    queue.push("screen-changed", 1);
    assert!(matches!(
        queue.admit("screen-changed", 2, None),
        QueueAdmission::Stuck(_)
    ));
    assert_eq!(queue.pending().next().unwrap().key, 1);
}

#[test]
fn first_loss_report_consumes_a_real_slot_even_in_capacity_one() {
    let mut queue = QueueModel::new(1);
    queue.admit("audio-frames", 1, Some("42"));
    assert_eq!(
        queue.admit("audio-frames", 2, Some("42")),
        QueueAdmission::Dropped
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.dropped_total(), 2);
    let batch = queue.take_batch(256);
    assert_eq!(batch.events[0].event, "events-dropped");
    assert_eq!(batch.dropped[0].count, 2);
    assert!(queue.is_empty());
    assert!(queue.events_dropped_records_with_sources().is_empty());
    assert_eq!(queue.dropped_total(), 2);
}

#[test]
fn zero_capacity_cannot_silently_lose_a_never_dropped_event_or_its_report() {
    let mut queue = QueueModel::new(0);
    assert!(matches!(
        queue.admit("node-removed", 1, None),
        QueueAdmission::Stuck(_)
    ));
    assert!(matches!(
        queue.admit("output-matched", 2, Some("filter")),
        QueueAdmission::Stuck(_)
    ));
    assert!(queue.is_empty());
    assert_eq!(queue.dropped_total(), 0);
}

#[test]
fn delivery_batches_are_bounded_and_do_not_clear_undelivered_reports() {
    let mut queue = QueueModel::new(300);
    for key in 0..300 {
        queue.admit("output-matched", key, Some("filter"));
    }
    queue.admit("output-matched", 300, Some("filter"));
    assert!(queue.take_batch(0).events.is_empty());
    assert!(!queue.events_dropped_records_with_sources().is_empty());
    let batch = queue.take_batch(usize::MAX);
    assert_eq!(batch.events.len(), 256);
    assert_eq!(batch.events[0].event, "events-dropped");
    assert_eq!(batch.dropped[0].count, 2);
    assert_eq!(queue.len(), 44);
    assert!(queue.events_dropped_records_with_sources().is_empty());
}

#[test]
fn counted_metadata_without_source_is_refused_without_changing_queue() {
    for name in ["output-matched", "topic-published", "audio-frames"] {
        let mut queue = QueueModel::new(1);
        assert!(
            matches!(queue.admit(name, 9, None), QueueAdmission::MissingSource(event) if event.event == name && event.key == 9)
        );
        assert!(
            matches!(queue.push(name, 10), QueueAdmission::MissingSource(event) if event.key == 10)
        );
        assert!(queue.is_empty());
        assert_eq!(queue.dropped_total(), 0);
        assert!(queue.events_dropped_records_with_sources().is_empty());
    }
}

#[test]
fn real_payload_sources_override_missing_and_incorrect_metadata() {
    use ilium_plugin_api::generated::types::{
        AudioFramesEvent, OutputMatchedEvent, TopicPublishedEvent,
    };
    let events = [
        (
            Event::OutputMatched(OutputMatchedEvent {
                filter_id: "actual-filter".into(),
                ..Default::default()
            }),
            "actual-filter",
        ),
        (
            Event::TopicPublished(TopicPublishedEvent {
                topic: "actual-topic".into(),
                ..Default::default()
            }),
            "actual-topic",
        ),
        (
            Event::AudioFrames(AudioFramesEvent {
                capture: 42,
                samples: vec![],
            }),
            "42",
        ),
    ];
    for (event, expected) in events {
        let mut queue = QueueModel::new(1);
        assert_eq!(queue.admit_event(&event, 1, None), QueueAdmission::Enqueued);
        assert_eq!(
            queue.pending().next().unwrap().source.as_deref(),
            Some(expected)
        );
        assert_eq!(
            queue.admit_event(&event, 2, Some("wrong")),
            QueueAdmission::Dropped
        );
        assert_eq!(
            queue.events_dropped_records_with_sources()[0]
                .source
                .as_deref(),
            Some(expected)
        );
        assert_eq!(queue.events_dropped_records_with_sources()[0].count, 2);
    }
}

#[test]
fn repeated_rejected_pushes_return_custody_without_retained_side_queue() {
    let mut queue = QueueModel::new(1);
    assert_eq!(queue.push("node-removed", 0), QueueAdmission::Enqueued);
    for key in 1..10000 {
        assert!(
            matches!(queue.push("pane-created", key), QueueAdmission::Stuck(event) if event.key == key)
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pending().next().unwrap().key, 0);
    }
    assert_eq!(queue.dropped_total(), 0);
}
