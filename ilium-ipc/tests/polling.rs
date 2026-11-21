use ilium_core::polling_detection::{DetectorStatus, PollingSettings};
use ilium_ipc::polling::{
    FindingFeedback, HistoryCursor, PollingCostEstimate, PollingHistoryPage, PollingOperation,
    PollingRejection, PollingReply, PollingSession, PollingSnapshot,
};

#[test]
fn polling_operations_round_trip_and_enforce_page_bounds() {
    let session = PollingSession {
        session_id: "synthetic-session".into(),
        incarnation: "synthetic-incarnation".into(),
    };
    let operations = [
        PollingOperation::Snapshot,
        PollingOperation::History {
            cursor: Some(HistoryCursor {
                revision: 3,
                after_sequence: 17,
            }),
            limit: 20,
        },
        PollingOperation::UpdateSettings {
            settings: PollingSettings::default(),
        },
        PollingOperation::Feedback {
            session: session.clone(),
            pane_id: ilium_core::NodeId(1),
            generation: 2,
            finding_id: "finding".into(),
            feedback: FindingFeedback::Unsure,
        },
        PollingOperation::Dismiss {
            session: session.clone(),
            pane_id: ilium_core::NodeId(1),
            generation: 2,
            finding_id: "finding".into(),
        },
        PollingOperation::EstimateRecent {
            session: session.clone(),
            pane_id: ilium_core::NodeId(1),
            generation: 2,
        },
        PollingOperation::AnalyzeRecent {
            session,
            pane_id: ilium_core::NodeId(1),
            generation: 2,
            estimate_id: "estimate".into(),
        },
    ];
    for operation in operations {
        operation.validate().unwrap();
        let encoded = bincode::serialize(&operation).unwrap();
        let decoded: PollingOperation = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded, operation);
    }
    assert!(PollingOperation::History {
        cursor: None,
        limit: 0
    }
    .validate()
    .is_err());
    assert!(PollingOperation::History {
        cursor: None,
        limit: 101
    }
    .validate()
    .is_err());
    assert!(PollingOperation::History {
        cursor: Some(HistoryCursor {
            revision: 0,
            after_sequence: 0
        }),
        limit: 1,
    }
    .validate()
    .is_err());
    assert!(PollingOperation::EstimateRecent {
        session: PollingSession {
            session_id: "session".into(),
            incarnation: String::new()
        },
        pane_id: ilium_core::NodeId(1),
        generation: 1,
    }
    .validate()
    .is_err());
}

#[test]
fn polling_replies_keep_unknown_pricing_and_stale_state_explicit() {
    let replies = [
        PollingReply::Snapshot(Box::new(PollingSnapshot {
            revision: 1,
            status: DetectorStatus::Off,
            settings: PollingSettings::default(),
            key_configured: false,
            findings: Vec::new(),
            day: None,
            month: None,
            all_time: None,
            coalesced: 0,
            dropped_capacity: 0,
        })),
        PollingReply::Accepted,
        PollingReply::Rejected(PollingRejection::StaleCursor),
        PollingReply::History(PollingHistoryPage {
            revision: 5,
            records: Vec::new(),
            next: None,
        }),
        PollingReply::Estimate(PollingCostEstimate {
            estimate_id: "estimate".into(),
            maximum_input_tokens: 3000,
            maximum_nanodollars: None,
            evidence_truncated: true,
        }),
    ];
    for reply in replies {
        let encoded = bincode::serialize(&reply).unwrap();
        let decoded: PollingReply = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded, reply);
    }
}
