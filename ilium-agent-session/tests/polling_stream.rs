use ilium_agent_session::polling_windows::{BatchApplication, PollingWindows};
use ilium_core::polling_detection::{
    ActivityEvent, ActivityKind, AgentBinding, PollingActivityBatch, PollingSettings,
};
use ilium_core::NodeId;
use std::time::Duration;

fn batch(start: u64, end: u64, message: bool) -> PollingActivityBatch {
    PollingActivityBatch {
        binding: AgentBinding {
            session: "synthetic-session".into(),
            incarnation: "synthetic-server".into(),
            pane: NodeId(7),
            generation: 1,
        },
        source_start: start,
        source_end: end,
        contains_gap: false,
        events: if message {
            vec![ActivityEvent {
                kind: ActivityKind::Message,
                timestamp: None,
                name: None,
                call_id: None,
                text: "Synthetic pending activity".into(),
                truncated: false,
                is_error: None,
            }]
        } else {
            vec![]
        },
    }
}

fn windows(interval: u8, spacing: u16) -> PollingWindows {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: interval,
        minimum_spacing_seconds: spacing,
        ..Default::default()
    });
    windows
}

#[test]
fn stream_requires_baseline_and_deduplicates_repeated_source_ranges() {
    let mut windows = windows(2, 0);
    assert_eq!(
        windows.apply_batch(batch(100, 200, true), Duration::ZERO),
        BatchApplication::Rejected
    );
    assert_eq!(
        windows.apply_batch(batch(100, 100, false), Duration::ZERO),
        BatchApplication::Baseline
    );
    assert_eq!(
        windows.apply_batch(batch(100, 200, true), Duration::ZERO),
        BatchApplication::Applied { ready: false }
    );
    assert_eq!(
        windows.apply_batch(batch(100, 200, true), Duration::ZERO),
        BatchApplication::Duplicate
    );
    assert_eq!(
        windows.apply_batch(batch(200, 300, true), Duration::ZERO),
        BatchApplication::Applied { ready: true }
    );
    assert_eq!(
        windows.events(&batch(0, 0, false).binding).unwrap().count(),
        2
    );
}

#[test]
fn stream_gap_discards_incomplete_evidence_without_inventing_interval_ticks() {
    let mut windows = windows(2, 0);
    assert_eq!(
        windows.apply_batch(batch(100, 100, false), Duration::ZERO),
        BatchApplication::Baseline
    );
    windows.apply_batch(batch(100, 200, true), Duration::ZERO);
    let mut missing = batch(300, 400, true);
    missing.contains_gap = true;
    assert_eq!(
        windows.apply_batch(missing, Duration::ZERO),
        BatchApplication::Gap
    );
    assert_eq!(
        windows.events(&batch(0, 0, false).binding).unwrap().count(),
        0
    );
    assert_eq!(
        windows.apply_batch(batch(400, 500, true), Duration::ZERO),
        BatchApplication::Applied { ready: false }
    );
    assert_eq!(
        windows.apply_batch(batch(500, 600, true), Duration::ZERO),
        BatchApplication::Applied { ready: true }
    );
}

#[test]
fn stream_gap_does_not_erase_the_last_paid_check_spacing() {
    let mut windows = windows(1, 10);
    windows.apply_batch(batch(100, 100, false), Duration::ZERO);
    assert_eq!(
        windows.apply_batch(batch(100, 200, true), Duration::ZERO),
        BatchApplication::Applied { ready: true }
    );
    assert!(windows.mark_checked(&batch(0, 0, false).binding, Duration::ZERO));
    assert_eq!(
        windows.apply_batch(batch(300, 400, true), Duration::from_secs(1)),
        BatchApplication::Gap
    );
    assert_eq!(
        windows.apply_batch(batch(400, 500, true), Duration::from_secs(2)),
        BatchApplication::Applied { ready: false }
    );
    assert_eq!(
        windows.apply_batch(batch(500, 600, true), Duration::from_secs(10)),
        BatchApplication::Applied { ready: true }
    );
}
