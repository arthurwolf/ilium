use ilium_core::{
    polling_detection::{ActivityEvent, ActivityKind, AgentBinding},
    NodeId,
};
use ilium_ipc::polling::PollingActivityBatch;

fn batch() -> PollingActivityBatch {
    PollingActivityBatch {
        binding: AgentBinding {
            session: "session".into(),
            incarnation: "incarnation".into(),
            pane: NodeId(7),
            generation: 2,
        },
        source_start: 100,
        source_end: 200,
        contains_gap: false,
        events: vec![ActivityEvent {
            kind: ActivityKind::Message,
            timestamp: None,
            name: None,
            call_id: None,
            text: "synthetic status".into(),
            truncated: false,
            is_error: None,
        }],
    }
}

#[test]
fn bounded_activity_batch_round_trips_with_identity_cursor_and_gap() {
    let mut original = batch();
    original.contains_gap = true;
    original.validate().unwrap();
    let encoded = bincode::serialize(&original).unwrap();
    let decoded: PollingActivityBatch = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn activity_feed_rejects_backwards_cursors_and_unbounded_records() {
    let mut value = batch();
    value.source_end = 99;
    assert!(value.validate().is_err());
    let mut value = batch();
    value.source_end = value.source_start;
    assert!(value.validate().is_err());
    let mut value = batch();
    value.binding.incarnation.clear();
    assert!(value.validate().is_err());
    let mut value = batch();
    value.binding.pane = ilium_core::ROOT_ID;
    assert!(value.validate().is_err());
    let mut value = batch();
    value.events[0].text = "x".repeat(1201);
    assert!(value.validate().is_err());
    let mut value = batch();
    value.events = vec![value.events[0].clone(); 513];
    assert!(value.validate().is_err());
}

#[test]
fn an_empty_baseline_carries_a_cursor_without_inventing_activity() {
    let mut value = batch();
    value.events.clear();
    value.source_end = value.source_start;
    value.validate().unwrap();
}

#[test]
fn private_polling_subscription_and_every_feed_event_round_trip() {
    use ilium_ipc::polling::{
        PollingEvidenceSubscription, PollingFeedEvent, PollingFeedStopReason,
    };
    use ilium_ipc::{ClientRequest, ServerEvent};
    let request = ClientRequest::SubscribePollingEvidence {
        request_id: 71,
        subscription: PollingEvidenceSubscription {
            expected_server_incarnation: "synthetic-incarnation".into(),
            leader_epoch: "synthetic-epoch".into(),
        },
    };
    let decoded: ClientRequest =
        bincode::deserialize(&bincode::serialize(&request).unwrap()).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(request.diagnostic_name(), "subscribe_polling_evidence");
    for event in [
        PollingFeedEvent::Batch(batch()),
        PollingFeedEvent::Closed(batch().binding),
        PollingFeedEvent::Stopped(PollingFeedStopReason::Disabled),
        PollingFeedEvent::Stopped(PollingFeedStopReason::AuthorityLost),
        PollingFeedEvent::Stopped(PollingFeedStopReason::Unavailable),
    ] {
        let original = ServerEvent::PollingEvidence {
            subscription_id: 71,
            event,
        };
        let decoded: ServerEvent =
            bincode::deserialize(&bincode::serialize(&original).unwrap()).unwrap();
        assert_eq!(decoded, original);
    }
}

#[test]
fn private_subscription_reply_names_its_server_and_charges_retained_identity() {
    use ilium_ipc::polling::PollingReply;
    let reply = PollingReply::EvidenceSubscribed {
        server_incarnation: "synthetic-server-identity".into(),
    };
    let event = ilium_ipc::ServerEvent::Polling {
        request_id: 71,
        reply,
    };
    let decoded: ilium_ipc::ServerEvent =
        bincode::deserialize(&bincode::serialize(&event).unwrap()).unwrap();
    assert_eq!(decoded, event);
    assert!(
        event.retained_bytes() >= std::mem::size_of_val(&event) + "synthetic-server-identity".len()
    );
}
