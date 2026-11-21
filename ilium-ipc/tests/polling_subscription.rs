use ilium_ipc::polling::{
    PollingFeedEvent, PollingFeedStopReason, PollingReply, SubscriptionGate,
    SubscriptionObservation,
};
use ilium_ipc::ServerEvent;

fn accepted(request_id: u64, incarnation: &str) -> ServerEvent {
    ServerEvent::Polling {
        request_id,
        reply: PollingReply::EvidenceSubscribed {
            server_incarnation: incarnation.into(),
        },
    }
}

#[test]
fn private_gate_requires_a_correlated_named_subscription_reply() {
    let mut gate = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        gate.observe(ServerEvent::Polling {
            request_id: 9,
            reply: PollingReply::Accepted
        }),
        SubscriptionObservation::Pending
    );
    assert_eq!(
        gate.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Subscribed
    );
    let mut legacy = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        legacy.observe(ServerEvent::Polling {
            request_id: 7,
            reply: PollingReply::Accepted
        }),
        SubscriptionObservation::Rejected
    );
}

#[test]
fn private_gate_cannot_be_revived_after_a_wrong_serving_identity() {
    let mut gate = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        gate.observe(accepted(7, "different-server")),
        SubscriptionObservation::Rejected
    );
    assert_eq!(
        gate.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Rejected
    );
    let mut valid = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        valid.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Subscribed
    );
}

#[test]
fn private_gate_delivers_only_its_bound_generation_stream_and_stops_once() {
    use ilium_core::polling_detection::{AgentBinding, PollingActivityBatch};
    let mut gate = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        gate.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Subscribed
    );
    let batch = PollingFeedEvent::Batch(PollingActivityBatch {
        binding: AgentBinding {
            session: "synthetic-session".into(),
            incarnation: "synthetic-server".into(),
            pane: ilium_core::NodeId(2),
            generation: 1,
        },
        source_start: 100,
        source_end: 100,
        contains_gap: false,
        events: vec![],
    });
    assert_eq!(
        gate.observe(ServerEvent::PollingEvidence {
            subscription_id: 7,
            event: batch.clone()
        }),
        SubscriptionObservation::Activity(batch)
    );
    let stopped = PollingFeedEvent::Stopped(PollingFeedStopReason::AuthorityLost);
    assert_eq!(
        gate.observe(ServerEvent::PollingEvidence {
            subscription_id: 7,
            event: stopped.clone()
        }),
        SubscriptionObservation::Activity(stopped)
    );
    assert_eq!(
        gate.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Rejected
    );
}

#[test]
fn private_gate_bounds_metadata_before_handshake_and_rejects_mismatched_feeds() {
    let mut gate = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    for _ in 0..64 {
        assert_eq!(
            gate.observe(ServerEvent::Polling {
                request_id: 9,
                reply: PollingReply::Accepted
            }),
            SubscriptionObservation::Pending
        );
    }
    assert_eq!(
        gate.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Subscribed
    );
    assert_eq!(
        gate.observe(ServerEvent::PollingEvidence {
            subscription_id: 8,
            event: PollingFeedEvent::Stopped(PollingFeedStopReason::Disabled)
        }),
        SubscriptionObservation::Rejected
    );
    let mut flooded = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    for _ in 0..64 {
        flooded.observe(ServerEvent::Polling {
            request_id: 9,
            reply: PollingReply::Accepted,
        });
    }
    assert_eq!(
        flooded.observe(ServerEvent::Polling {
            request_id: 9,
            reply: PollingReply::Accepted
        }),
        SubscriptionObservation::Rejected
    );
    assert_eq!(
        flooded.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Rejected
    );
}

#[test]
fn private_gate_rejects_foreign_closures_invalid_ranges_and_premature_evidence() {
    use ilium_core::polling_detection::{AgentBinding, PollingActivityBatch};
    let binding = AgentBinding {
        session: "synthetic-session".into(),
        incarnation: "foreign-server".into(),
        pane: ilium_core::NodeId(2),
        generation: 1,
    };
    for event in [
        PollingFeedEvent::Closed(binding.clone()),
        PollingFeedEvent::Batch(PollingActivityBatch {
            binding: binding.clone(),
            source_start: 100,
            source_end: 100,
            contains_gap: false,
            events: vec![],
        }),
        PollingFeedEvent::Batch(PollingActivityBatch {
            binding: AgentBinding {
                incarnation: "synthetic-server".into(),
                ..binding
            },
            source_start: 200,
            source_end: 100,
            contains_gap: false,
            events: vec![],
        }),
    ] {
        let mut gate = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
        assert_eq!(
            gate.observe(accepted(7, "synthetic-server")),
            SubscriptionObservation::Subscribed
        );
        assert_eq!(
            gate.observe(ServerEvent::PollingEvidence {
                subscription_id: 7,
                event
            }),
            SubscriptionObservation::Rejected
        );
        assert_eq!(
            gate.observe(accepted(7, "synthetic-server")),
            SubscriptionObservation::Rejected
        );
    }
    let mut premature = SubscriptionGate::new(7, "synthetic-server".into()).unwrap();
    assert_eq!(
        premature.observe(ServerEvent::PollingEvidence {
            subscription_id: 7,
            event: PollingFeedEvent::Stopped(PollingFeedStopReason::Disabled)
        }),
        SubscriptionObservation::Rejected
    );
    assert_eq!(
        premature.observe(accepted(7, "synthetic-server")),
        SubscriptionObservation::Rejected
    );
}
