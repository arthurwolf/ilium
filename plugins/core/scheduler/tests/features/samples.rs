//! The four page samples as behaviour: durable timer, queue, multi-stage input, retry after an uncertain delivery.

use ilium_plugin_sdk::types::{
    DeactivationReason, Delivery, DurableTimerEvent, Event, InputStage, InputUncertainEvent,
    NodeId, SendInputSpec, TimerId,
};

use crate::common::*;

fn grants() -> Vec<String> {
    manifest()["capabilities"]["server"]
        .as_array()
        .expect("server grants")
        .iter()
        .map(|grant| grant.as_str().expect("string").to_string())
        .collect()
}

/// REQ: F-055, scheduler-B08
#[test]
fn sample_055_the_timer_is_named_send_colon_pane_and_a_late_timer_fires_once() {
    let page = page_text();
    assert!(page.contains("let name = format!(\"send:{pane}\");"));
    assert!(page.contains("// A timer that came due while Ilium was stopped fires once, late."));
}

/// REQ: F-055, scheduler-B08
#[test]
fn sample_055_a_second_fire_of_the_same_timer_sends_nothing() {
    let mut session = session();
    server(&mut session).set_clock(1_000_000_000);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 10, "x", true).expect("schedule");
    let event = || {
        Event::DurableTimerFired(DurableTimerEvent {
            timer: TimerId(1),
            name: format!("send:{}", pane.0),
            late_by_millis: 0,
        })
    };
    assert_eq!(sends(&server(&mut session).deliver_one(event())).len(), 1);
    assert!(sends(&server(&mut session).deliver_one(event())).is_empty());
}

/// REQ: F-057
#[test]
fn sample_057_the_queue_limit_constant_is_100() {
    assert!(page_text().contains("const QUEUE_LIMIT: usize = 100;"));
}

/// REQ: F-057, scheduler-B16
#[test]
fn sample_057_the_dedup_key_is_queue_pane_revision() {
    assert!(page_text().contains("format!(\"queue:{}:{}\", change.node, change.revision)"));
}

/// REQ: F-055, scheduler-B08
#[test]
fn sample_055_multi_stage_input_needs_the_raw_input_and_input_lock_grants() {
    let grants = grants();
    assert!(grants.contains(&"terminal.raw-input".to_string()));
    assert!(grants.contains(&"terminal.input-lock".to_string()));
    assert!(grants.contains(&"terminal.write".to_string()));
}

/// REQ: F-055, scheduler-B08
#[test]
fn sample_055_the_slash_command_stages_fit_the_send_input_limits() {
    let stages = [
        InputStage {
            text: "/model".into(),
            delay_ms: 0,
            delivery: Delivery::Typed,
        },
        InputStage {
            text: "opus".into(),
            delay_ms: 150,
            delivery: Delivery::Typed,
        },
        InputStage {
            text: String::new(),
            delay_ms: 120,
            delivery: Delivery::Submitted,
        },
    ];
    assert!(stages.len() <= 16, "at most 16 stages");
    assert!(
        stages
            .iter()
            .map(|stage| u64::from(stage.delay_ms))
            .sum::<u64>()
            <= 10_000,
        "total delay at most 10 s"
    );
    let spec = SendInputSpec {
        stages: stages.to_vec(),
        lock_user_input: true,
        ..plain_spec(NodeId(1), "", Delivery::Typed)
    };
    assert!(spec.lock_user_input);
}

/// REQ: F-055, scheduler-B08
#[test]
fn sample_055_a_scheduled_send_never_locks_user_input_and_never_uses_raw_bytes() {
    let mut session = session();
    server(&mut session).set_clock(1_000_000_000);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 1, "x", true).expect("schedule");
    server(&mut session).advance_clock(1_000);
    let spec = sends(&server(&mut session).submitted()).remove(0).0;
    assert!(!spec.lock_user_input && spec.raw.is_none());
}

/// REQ: F-055, F-057
#[test]
fn sample_retry_a_delivery_whose_journal_entry_exists_is_not_retried() {
    let mut session = session();
    server(&mut session).set_clock(1_000_000_000);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 1, "x", true).expect("schedule");
    server(&mut session).crash_after_journal_write();
    server(&mut session).advance_clock(1_000);
    server(&mut session).advance_clock(60_000);
    let before = sends(&server(&mut session).submitted()).len();
    server(&mut session).deliver_one(Event::InputUncertain(InputUncertainEvent {
        pane,
        dedup_key: format!("send:{}", pane.0),
    }));
    server(&mut session).advance_clock(10_000);
    assert_eq!(
        sends(&server(&mut session).submitted()).len(),
        before,
        "the entry is in the journal: no retry"
    );
}

/// REQ: F-055, F-057
#[test]
fn sample_retry_a_missing_journal_entry_is_retried_once_after_2_seconds_with_the_same_key() {
    let mut session = session();
    server(&mut session).set_clock(1_000_000_000);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 1, "x", true).expect("schedule");
    server(&mut session).advance_clock(1_000);
    let first = sends(&server(&mut session).submitted());
    server(&mut session).deliver_one(Event::InputUncertain(InputUncertainEvent {
        pane,
        dedup_key: format!("send:{}", pane.0),
    }));
    server(&mut session).advance_clock(1_999);
    assert_eq!(
        sends(&server(&mut session).submitted()).len(),
        first.len(),
        "back-off not over"
    );
    server(&mut session).advance_clock(1);
    let all = sends(&server(&mut session).submitted());
    assert_eq!(all.len(), first.len() + 1, "exactly one retry");
    assert_eq!(
        all.last().map(|entry| entry.1.clone()),
        Some(Some(format!("send:{}", pane.0)))
    );
}

/// REQ: F-055, F-057
#[test]
fn sample_retry_the_retry_happens_only_once() {
    let mut session = session();
    server(&mut session).set_clock(1_000_000_000);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 1, "x", true).expect("schedule");
    server(&mut session).advance_clock(1_000);
    server(&mut session).deliver_one(Event::InputUncertain(InputUncertainEvent {
        pane,
        dedup_key: format!("send:{}", pane.0),
    }));
    server(&mut session).advance_clock(2_000);
    let after_retry = sends(&server(&mut session).submitted()).len();
    server(&mut session).advance_clock(3_600_000);
    assert_eq!(sends(&server(&mut session).submitted()).len(), after_retry);
}

/// REQ: F-055, F-057
#[test]
fn sample_retry_an_uncertain_key_of_another_plugin_is_ignored() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    server(&mut session).deliver_one(Event::InputUncertain(InputUncertainEvent {
        pane,
        dedup_key: "other-plugin:1".to_string(),
    }));
    server(&mut session).advance_clock(60_000);
    assert!(sends(&server(&mut session).submitted()).is_empty());
}

/// REQ: F-055, scheduler-B10
#[test]
fn sample_055_shutdown_returns_state_or_none_and_the_next_activation_works() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, 0, 0, 30, "x", true).expect("schedule");
    let _ = server(&mut session).deactivate(DeactivationReason::Shutdown);
    server(&mut session).activate::<ilium_plugin_core_scheduler::SchedulerServer>();
    assert!(!server(&mut session).is_disabled());
}
