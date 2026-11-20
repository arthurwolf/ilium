//! A failed write-ahead fence must never admit a prompt to the terminal.

use ilium_plugin_core_scheduler::{queue::Delivery, wire, SchedulerServer, PLUGIN_ID};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, DeactivationReason, ErrorCode, Event, Intent, MessageEvent,
    NodeId, PluginError, PluginId, PluginSide, TimerId,
};

#[test]
fn storage_refusal_prevents_queued_prompt_delivery() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::EnqueueRequest {
        pane: 7,
        text: "x".repeat(1024 * 1024),
        delivery: Delivery::Once,
    };
    let response = host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    assert!(!response.is_empty(), "the enqueue request must be handled");
    assert!(response.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))), "a refused enqueue must be reported");
    assert!(host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state").is_none(), "a refused enqueue must not claim to be saved");
    let submissions = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 1,
    })]);
    assert!(
        submissions
            .iter()
            .all(|submission| !matches!(&submission.intent, Intent::SendInput(_))),
        "a refused persistence fence must prevent terminal input"
    );
}

#[test]
fn countdown_waits_for_persistent_removal_before_sending() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::ScheduleRequest {
        pane: 7,
        delay_seconds: 1,
        text: "continue".to_owned(),
        send_enter: true,
    };
    host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::SCHEDULE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    let saved_state = host
        .storage()
        .get(ilium_plugin_sdk::host::Bucket::Project, "state")
        .expect("the schedule was saved");
    host.storage_mut().set_write_error(Some(PluginError::new(
        ErrorCode::Internal,
        "injected storage failure",
    )));
    let before = host.submitted().len();
    host.advance_clock(1_000);
    let submissions = host.submitted()[before..].to_vec();
    assert!(submissions
        .iter()
        .all(|submission| !matches!(&submission.intent, Intent::SendInput(_))));
    assert_eq!(
        host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"),
        Some(saved_state)
    );
    assert!(submissions.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))));
    let retry_at = host
        .durable_timers()
        .into_iter()
        .find_map(|(name, due)| (name == "scheduled-7").then_some(due))
        .expect("the failed delivery must rearm the countdown");
    assert!(retry_at > host.clock().now(), "retry must be scheduled in the future");
    host.storage_mut().set_write_error(None);
    let before = host.submitted().len();
    host.advance_clock(retry_at - host.clock().now());
    let retry = host.submitted()[before..].to_vec();
    assert!(retry
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::SendInput(_))));
}

#[test]
fn transient_storage_failure_preserves_the_queue_without_sending() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::EnqueueRequest {
        pane: 7,
        text: "continue".to_owned(),
        delivery: Delivery::Once,
    };
    host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    let before = host
        .storage()
        .get(ilium_plugin_sdk::host::Bucket::Project, "state")
        .expect("the queue was saved");
    host.storage_mut().set_write_error(Some(PluginError::new(
        ErrorCode::Internal,
        "injected storage failure",
    )));
    let submissions = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 1,
    })]);
    assert!(submissions
        .iter()
        .all(|submission| !matches!(&submission.intent, Intent::SendInput(_))));
    assert_eq!(
        host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"),
        Some(before)
    );
    assert!(submissions.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))));
    host.storage_mut().set_write_error(None);
    host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 2,
    })]);
    let retry = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 3,
    })]);
    assert!(retry
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::SendInput(_))));
}

#[test]
fn refused_cancellation_keeps_the_saved_countdown_active() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::ScheduleRequest {
        pane: 7,
        delay_seconds: 1,
        text: "continue".to_owned(),
        send_enter: true,
    };
    host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::SCHEDULE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    host.storage_mut().set_write_error(Some(PluginError::new(
        ErrorCode::Internal,
        "injected storage failure",
    )));
    let response = host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::CANCEL_SCHEDULE_CHANNEL.to_owned(),
        payload: wire::pane_payload(7),
    })]);
    assert!(response.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))));
    assert!(host.durable_timers().iter().any(|(name, _)| name == "scheduled-7"));
    host.storage_mut().set_write_error(None);
    let before = host.submitted().len();
    host.advance_clock(1_000);
    let due = host.submitted()[before..].to_vec();
    assert!(due.iter().any(|submission| matches!(&submission.intent, Intent::SendInput(_))));
}

#[test]
fn activation_rearms_an_overdue_saved_countdown_at_a_valid_time() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    let deadline = host.clock().now().saturating_add(1_000);
    let mut stored = wire::Stored::default();
    stored.schedules.insert(
        7,
        ilium_plugin_core_scheduler::schedule::ScheduledInput {
            execute_at_unix_millis: deadline,
            text: "continue".to_owned(),
            send_enter: true,
        },
    );
    host.storage_mut().put(
        ilium_plugin_sdk::host::Bucket::Project,
        "state",
        &wire::encode_stored(&stored),
    );
    host.advance_clock(5_000);
    host.activate::<SchedulerServer>();
    let retry_at = host
        .durable_timers()
        .into_iter()
        .find_map(|(name, due)| (name == "scheduled-7").then_some(due))
        .expect("the saved countdown must be rearmed");
    assert!(retry_at >= host.clock().now(), "the host refuses stale deadlines");
}

#[test]
fn missing_delivery_marker_never_replays_an_uncertain_prompt() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    let damaged = br#"{"schedules":[],"queues":[{"pane":7,"prompts":[{"text":"continue","delivery":{"mode":"once"}}]}]}"#;
    host.storage_mut().put(ilium_plugin_sdk::host::Bucket::Project, "state", damaged);
    host.activate::<SchedulerServer>();
    let response = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 1,
    })]);
    assert!(response.iter().all(|submission| !matches!(&submission.intent, Intent::SendInput(_))));
    assert_eq!(host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"), Some(damaged.to_vec()));
}

#[test]
fn a_persisted_delivery_attempt_is_not_replayed_after_reactivation() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::EnqueueRequest {
        pane: 7,
        text: "continue".to_owned(),
        delivery: Delivery::Once,
    };
    host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    let finish = || Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 1,
    });
    let first = host.deliver(vec![finish()]);
    assert!(first.iter().any(|submission| matches!(&submission.intent, Intent::SendInput(_))));
    host.deactivate(DeactivationReason::Shutdown);
    host.activate::<SchedulerServer>();
    let after_restart = host.deliver(vec![finish()]);
    assert!(after_restart.iter().all(|submission| !matches!(&submission.intent, Intent::SendInput(_))));
}

#[test]
fn malformed_storage_is_not_overwritten_by_a_new_queue_request() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.storage_mut().put(ilium_plugin_sdk::host::Bucket::Project, "state", b"{");
    host.activate::<SchedulerServer>();
    let request = wire::EnqueueRequest {
        pane: 7,
        text: "continue".to_owned(),
        delivery: Delivery::Once,
    };
    let response = host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(),
        payload: request.to_payload(),
    })]);
    assert!(response.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))));
    assert_eq!(host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"), Some(b"{".to_vec()));
}

#[test]
fn unreadable_saved_state_is_not_overwritten() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest).expect("manifest loads").side("server").build();
    let original = wire::encode_stored(&wire::Stored::default());
    host.storage_mut().put(ilium_plugin_sdk::host::Bucket::Project, "state", &original);
    host.storage_mut().set_read_error(Some(PluginError::new(ErrorCode::Internal, "storage unavailable")));
    host.activate::<SchedulerServer>();
    let request = wire::EnqueueRequest { pane: 7, text: "continue".to_owned(), delivery: Delivery::Once };
    let response = host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()), from_side: PluginSide::Client, from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(), payload: request.to_payload(),
    })]);
    assert!(response.iter().any(|submission| matches!(&submission.intent, Intent::Toast(_))));
    assert_eq!(host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"), Some(original));
}

#[test]
fn scheduler_recovers_after_a_transient_initial_read_failure() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest).expect("manifest loads").side("server").build();
    let original = wire::encode_stored(&wire::Stored::default());
    host.storage_mut().put(ilium_plugin_sdk::host::Bucket::Project, "state", &original);
    host.storage_mut().set_read_error(Some(PluginError::new(ErrorCode::Internal, "storage unavailable")));
    host.activate::<SchedulerServer>();
    host.storage_mut().set_read_error(None);
    let request = wire::EnqueueRequest { pane: 7, text: "continue".to_owned(), delivery: Delivery::Once };
    let submissions = host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()), from_side: PluginSide::Client, from_client: None,
        channel: wire::ENQUEUE_CHANNEL.to_owned(), payload: request.to_payload(),
    })]);
    assert!(submissions.iter().all(|item| !matches!(&item.intent, Intent::Toast(_))),
        "a recovered storage read should allow the request");
    assert_ne!(host.storage().get(ilium_plugin_sdk::host::Bucket::Project, "state"), Some(original),
        "the recovered request must be persisted");
}

#[test]
fn a_failed_retry_timer_is_rearmed_after_the_host_recovers() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SchedulerServer>();
    let request = wire::ScheduleRequest { pane: 7, delay_seconds: 1, text: "continue".to_owned(), send_enter: true };
    host.deliver(vec![Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_owned()), from_side: PluginSide::Client, from_client: None,
        channel: wire::SCHEDULE_CHANNEL.to_owned(), payload: request.to_payload(),
    })]);
    host.storage_mut().set_write_error(Some(PluginError::new(ErrorCode::Internal, "storage failure")));
    host.set_durable_timer_error(Some(PluginError::new(ErrorCode::Internal, "timer failure")));
    host.advance_clock(1_000);
    host.storage_mut().set_write_error(None);
    host.set_durable_timer_error(None);
    let ticker = host.active_timers().into_iter().find_map(|(id, repeat)| (repeat == Some(1000)).then_some(id)).expect("countdown ticker");
    host.deliver(vec![Event::TimerFired(TimerId(u64::from(ticker)))]);
    let due = host.durable_timers().into_iter().find_map(|(name, due)| (name == "scheduled-7").then_some(due)).expect("retry rearmed");
    assert!(due > host.clock().now());
}
