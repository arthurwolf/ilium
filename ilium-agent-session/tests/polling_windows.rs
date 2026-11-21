use ilium_agent_session::polling_windows::PollingWindows;
use ilium_core::polling_detection::{ActivityEvent, ActivityKind, AgentBinding};
use ilium_core::{polling_detection::PollingSettings, NodeId};
use std::time::Duration;

fn binding(generation: u64) -> AgentBinding {
    AgentBinding {
        session: "session".into(),
        incarnation: "incarnation".into(),
        pane: NodeId(7),
        generation,
    }
}

fn message() -> ActivityEvent {
    ActivityEvent {
        kind: ActivityKind::Message,
        timestamp: None,
        name: None,
        call_id: None,
        text: "still running".into(),
        truncated: false,
        is_error: None,
    }
}

#[test]
fn disabled_collection_does_not_create_a_retrospective_burst() {
    let mut windows = PollingWindows::default();
    let mut settings = PollingSettings {
        event_interval: 2,
        minimum_spacing_seconds: 0,
        ..Default::default()
    };
    windows.configure(&settings);
    windows.bind(binding(1));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    settings.enabled = true;
    windows.configure(&settings);
    windows.bind(binding(1));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(windows.observe(&binding(1), message(), Duration::ZERO));
}

#[test]
fn events_from_a_replaced_generation_cannot_enter_the_new_window() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 2,
        minimum_spacing_seconds: 0,
        ..Default::default()
    });
    windows.bind(binding(1));
    windows.bind(binding(2));
    windows.bind(binding(1));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(!windows.observe(&binding(2), message(), Duration::ZERO));
    assert!(windows.observe(&binding(2), message(), Duration::ZERO));
}

#[test]
fn checks_respect_spacing_and_require_new_activity() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 1,
        minimum_spacing_seconds: 10,
        ..Default::default()
    });
    windows.bind(binding(1));
    assert!(windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(windows.mark_checked(&binding(1), Duration::ZERO));
    assert!(!windows.mark_checked(&binding(1), Duration::from_secs(10)));
    assert!(!windows.observe(&binding(1), message(), Duration::from_secs(9)));
    assert!(windows.observe(&binding(1), message(), Duration::from_secs(10)));
    assert!(windows.mark_checked(&binding(1), Duration::from_secs(10)));
    assert!(!windows.mark_checked(&binding(2), Duration::from_secs(20)));
}

#[test]
fn invalid_configuration_discards_pending_activity() {
    let mut windows = PollingWindows::default();
    let mut settings = PollingSettings {
        enabled: true,
        event_interval: 2,
        minimum_spacing_seconds: 0,
        ..Default::default()
    };
    windows.configure(&settings);
    windows.bind(binding(1));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    settings.event_interval = 0;
    windows.configure(&settings);
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    settings.event_interval = 2;
    windows.configure(&settings);
    windows.bind(binding(1));
    assert!(!windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(windows.observe(&binding(1), message(), Duration::ZERO));
}

#[test]
fn capacity_rejection_is_counted_without_evicting_existing_windows() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 1,
        minimum_spacing_seconds: 0,
        ..Default::default()
    });
    for pane in 0..1024 {
        let mut agent = binding(1);
        agent.pane = NodeId(pane);
        assert!(windows.bind(agent));
    }
    let mut overflow = binding(1);
    overflow.pane = NodeId(1024);
    assert!(!windows.bind(overflow.clone()));
    assert_eq!(windows.dropped_capacity(), 1);
    assert!(windows.observe(&binding(1), message(), Duration::ZERO));
    assert!(windows.unbind(&binding(1)));
    assert!(windows.bind(overflow));
}

#[test]
fn closed_generation_releases_its_window_without_removing_a_replacement() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 1,
        minimum_spacing_seconds: 0,
        ..Default::default()
    });
    assert!(windows.bind(binding(1)));
    assert!(windows.bind(binding(2)));
    assert!(!windows.unbind(&binding(1)));
    assert!(windows.observe(&binding(2), message(), Duration::ZERO));
    assert!(windows.unbind(&binding(2)));
    assert!(!windows.observe(&binding(2), message(), Duration::ZERO));
}

#[test]
fn equal_session_names_and_pane_numbers_on_distinct_servers_keep_separate_windows() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 2,
        minimum_spacing_seconds: 0,
        ..Default::default()
    });
    let first = binding(1);
    let mut second = binding(1);
    second.incarnation = "another-live-server".into();
    assert!(windows.bind(first.clone()));
    assert!(windows.bind(second.clone()));
    assert!(!windows.observe(&first, message(), Duration::ZERO));
    assert!(!windows.observe(&second, message(), Duration::ZERO));
    assert!(windows.observe(&first, message(), Duration::ZERO));
    assert!(windows.observe(&second, message(), Duration::ZERO));
    assert!(windows.unbind(&first));
    assert!(windows.mark_checked(&second, Duration::ZERO));
}

#[test]
fn deferred_check_preserves_events_arriving_after_its_captured_prefix() {
    let mut windows = PollingWindows::default();
    windows.configure(&PollingSettings {
        enabled: true,
        event_interval: 5,
        minimum_spacing_seconds: 0,
        ..Default::default()
    });
    let agent = binding(1);
    assert!(windows.bind(agent.clone()));
    for _ in 0..5 {
        windows.observe(&agent, message(), Duration::ZERO);
    }
    let first = windows
        .capture_check_mark(&agent, Duration::ZERO)
        .expect("ready prefix has an interval mark");
    for _ in 0..2 {
        windows.observe(&agent, message(), Duration::ZERO);
    }
    assert!(windows.mark_checked_through(&first, Duration::ZERO));
    assert!(windows.capture_check_mark(&agent, Duration::ZERO).is_none());
    for _ in 0..3 {
        windows.observe(&agent, message(), Duration::ZERO);
    }
    let second = windows
        .capture_check_mark(&agent, Duration::ZERO)
        .expect("two deferred plus three new events reach the next interval");
    assert!(
        !windows.mark_checked_through(&first, Duration::ZERO),
        "duplicate admission must not reset spacing or counters"
    );
    assert!(windows.mark_checked_through(&second, Duration::ZERO));
    assert!(windows.capture_check_mark(&agent, Duration::ZERO).is_none());
}

#[test]
fn deferred_check_marks_cannot_follow_gaps_rebinding_or_configuration_reset() {
    use ilium_core::polling_detection::PollingActivityBatch;
    let mut windows = PollingWindows::default();
    let mut settings = PollingSettings {
        enabled: true,
        event_interval: 1,
        minimum_spacing_seconds: 0,
        ..Default::default()
    };
    windows.configure(&settings);
    let agent = binding(1);
    windows.apply_batch(
        PollingActivityBatch {
            binding: agent.clone(),
            source_start: 0,
            source_end: 0,
            contains_gap: false,
            events: vec![],
        },
        Duration::ZERO,
    );
    windows.observe(&agent, message(), Duration::ZERO);
    let before_gap = windows
        .capture_check_mark(&agent, Duration::ZERO)
        .expect("ready prefix");
    windows.apply_batch(
        PollingActivityBatch {
            binding: agent.clone(),
            source_start: 0,
            source_end: 1,
            contains_gap: true,
            events: vec![],
        },
        Duration::ZERO,
    );
    windows.observe(&agent, message(), Duration::ZERO);
    assert!(!windows.mark_checked_through(&before_gap, Duration::ZERO));
    let before_rebind = windows.capture_check_mark(&agent, Duration::ZERO).unwrap();
    assert!(windows.unbind(&agent));
    assert!(windows.bind(agent.clone()));
    windows.observe(&agent, message(), Duration::ZERO);
    assert!(!windows.mark_checked_through(&before_rebind, Duration::ZERO));
    let before_config = windows.capture_check_mark(&agent, Duration::ZERO).unwrap();
    settings.enabled = false;
    windows.configure(&settings);
    settings.enabled = true;
    windows.configure(&settings);
    assert!(windows.bind(agent.clone()));
    windows.observe(&agent, message(), Duration::ZERO);
    assert!(!windows.mark_checked_through(&before_config, Duration::ZERO));
    let current = windows.capture_check_mark(&agent, Duration::ZERO).unwrap();
    assert!(windows.mark_checked_through(&current, Duration::ZERO));
}
