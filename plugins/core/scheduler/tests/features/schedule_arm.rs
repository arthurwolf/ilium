//! Scheduled input on the server (F-055): the durable timer, the send-input intent, replacement,
//! restart, pane close and the deadline rules scheduler-B07 to B10.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{
    AgentActivity, DeactivationReason, Delivery, DurableTimerEvent, ErrorCode, Event, NodeId, Outcome,
    PaneLifecycleEvent, PaneLifecycleState, PluginError, TimerId,
};
use proptest::prelude::*;

use crate::common::*;

const START_MS: u64 = 1_000_000_000;

fn armed(h: u64, m: u64, s: u64, payload: &str, send_enter: bool) -> (FakeSession, NodeId) {
    let mut session = session();
    server(&mut session).set_clock(START_MS);
    let pane = terminal_pane(&mut session, "claude-api");
    schedule(&mut session, pane, h, m, s, payload, send_enter).expect("schedule is accepted");
    (session, pane)
}

fn sent_after(session: &mut FakeSession, ms: u64) -> usize {
    server(session).advance_clock(ms);
    sends(&server(session).submitted()).len()
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_nothing_is_sent_before_the_deadline_and_one_send_at_it() {
    let (mut session, _pane) = armed(2, 0, 30, "npm test", true);
    assert_eq!(sent_after(&mut session, 7_229_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1);
}

/// REQ: F-055, scheduler-B04
#[test]
fn arm_055_the_delay_is_hours_times_3600_plus_minutes_times_60_plus_seconds() {
    for (h, m, s, expected_ms) in [(0, 0, 1, 1_000u64), (0, 1, 0, 60_000), (1, 0, 0, 3_600_000), (1, 59, 59, 7_199_000), (24, 0, 0, 86_400_000)] {
        let (mut session, _pane) = armed(h, m, s, "x", true);
        assert_eq!(sent_after(&mut session, expected_ms - 1), 0, "{h}:{m}:{s} early");
        assert_eq!(sent_after(&mut session, 1), 1, "{h}:{m}:{s} at the deadline");
    }
}

proptest! {
    /// REQ: F-055, scheduler-B02
    #[test]
    fn arm_055_prop_the_deadline_is_exactly_the_countdown(h in 0u64..48, m in 0u64..60, s in 0u64..60) {
        prop_assume!(h + m + s > 0);
        let (mut session, _pane) = armed(h, m, s, "x", true);
        let delay_ms = ((h * 60 + m) * 60 + s) * 1000;
        prop_assert_eq!(sent_after(&mut session, delay_ms - 1), 0);
        prop_assert_eq!(sent_after(&mut session, 1), 1);
    }
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_text_and_enter_is_one_submitted_send_with_the_documented_shape() {
    let (mut session, pane) = armed(0, 0, 30, "npm test", true);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "npm test", Delivery::Submitted));
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_text_only_is_typed_without_enter() {
    let (mut session, pane) = armed(0, 0, 30, "npm test", false);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "npm test", Delivery::Typed));
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_enter_only_is_a_single_enter_with_empty_text() {
    let (mut session, pane) = armed(0, 0, 30, "", true);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "", Delivery::Submitted));
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_the_send_carries_the_dedup_key_send_colon_pane() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].1, Some(format!("send:{}", pane.0)));
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_the_send_is_journaled_and_has_no_stages_raw_or_precondition() {
    let (mut session, _pane) = armed(0, 0, 30, "x", true);
    server(&mut session).advance_clock(30_000);
    let spec = sends(&server(&mut session).submitted()).remove(0).0;
    assert!(spec.journal);
    assert!(spec.stages.is_empty() && spec.raw.is_none() && spec.mouse.is_none() && spec.if_current.is_none());
    assert_eq!(spec.prompt_epoch, None);
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_the_schedule_fires_once_and_not_again() {
    let (mut session, _pane) = armed(0, 0, 30, "x", true);
    assert_eq!(sent_after(&mut session, 30_000), 1);
    assert_eq!(sent_after(&mut session, 3_600_000), 1, "no second send");
}

/// REQ: F-055, scheduler-B07
#[test]
fn arm_055_a_new_schedule_on_the_same_pane_replaces_the_old_deadline() {
    let (mut session, pane) = armed(0, 0, 10, "old", true);
    schedule(&mut session, pane, 0, 0, 30, "new", true).expect("replaces");
    assert_eq!(sent_after(&mut session, 10_000), 0, "the old deadline is gone");
    assert_eq!(sent_after(&mut session, 20_000), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0.text, "new");
}

/// REQ: F-055, scheduler-B07
#[test]
fn arm_055_a_replacement_may_move_the_deadline_earlier() {
    let (mut session, pane) = armed(1, 0, 0, "old", true);
    schedule(&mut session, pane, 0, 0, 5, "soon", true).expect("replaces");
    assert_eq!(sent_after(&mut session, 5_000), 1);
}

/// REQ: F-055, scheduler-B07
#[test]
fn arm_055_two_panes_each_keep_their_own_schedule() {
    let mut session = session();
    server(&mut session).set_clock(START_MS);
    let first = terminal_pane(&mut session, "one");
    let second = terminal_pane(&mut session, "two");
    schedule(&mut session, first, 0, 0, 10, "a", true).expect("first");
    schedule(&mut session, second, 0, 0, 20, "b", true).expect("second");
    server(&mut session).advance_clock(20_000);
    let mut texts: Vec<String> = sends(&server(&mut session).submitted()).into_iter().map(|(spec, _)| spec.text).collect();
    texts.sort();
    assert_eq!(texts, ["a", "b"]);
}

/// REQ: F-055, scheduler-B07
#[test]
fn arm_055_the_list_shows_one_pending_schedule_per_pane() {
    let (mut session, pane) = armed(0, 0, 10, "old", true);
    schedule(&mut session, pane, 0, 0, 30, "new", true).expect("replaces");
    let reply = call_queue(&mut session, serde_json::json!({"method": "pending", "pane": pane.0})).expect("pending answers");
    assert_eq!(reply["schedules"].as_array().map(Vec::len), Some(1));
}

/// REQ: F-055, scheduler-B04
#[test]
fn arm_055_a_zero_delay_through_the_service_is_refused_with_the_message() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let error = schedule(&mut session, pane, 0, 0, 0, "x", true).expect_err("zero delay is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.message, "Choose a delay of at least one second");
}

/// REQ: F-055, scheduler-B05
#[test]
fn arm_055_empty_payload_without_enter_through_the_service_is_refused() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let error = schedule(&mut session, pane, 0, 0, 30, "", false).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.message, "Enter text, enable Send Enter, or use both");
}

/// REQ: F-055, scheduler-B06
#[test]
fn arm_055_minutes_or_seconds_above_59_are_refused_through_the_service() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    for (m, s) in [(60, 0), (0, 60), (99, 1)] {
        let error = schedule(&mut session, pane, 0, m, s, "x", true).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{m}:{s}");
    }
}

/// REQ: F-055, scheduler-B01
#[test]
fn arm_055_a_non_terminal_pane_cannot_be_scheduled() {
    let mut session = session();
    let pane = add_pane(&mut session, "notes.md", EDITOR_KIND);
    let error = schedule(&mut session, pane, 0, 0, 30, "x", true).expect_err("refused");
    assert!(matches!(error.code, ErrorCode::InvalidArgument | ErrorCode::Unsupported | ErrorCode::NotFound));
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_closing_the_pane_discards_the_schedule() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).tree().remove(pane.0);
    assert_eq!(sent_after(&mut session, 60_000), 0);
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_a_node_removed_event_discards_the_schedule() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).deliver_one(Event::NodeRemoved(pane));
    assert_eq!(sent_after(&mut session, 60_000), 0);
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_a_stray_timer_for_a_removed_pane_sends_nothing() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).deliver_one(Event::NodeRemoved(pane));
    let submissions = server(&mut session).deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: format!("send:{}", pane.0),
        late_by_millis: 0,
    }));
    assert!(sends(&submissions).is_empty());
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_a_timer_with_a_foreign_name_is_ignored() {
    let (mut session, _pane) = armed(0, 0, 30, "x", true);
    let submissions = server(&mut session).deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(9),
        name: "other:1".to_string(),
        late_by_millis: 0,
    }));
    assert!(sends(&submissions).is_empty());
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_the_deadline_survives_a_restart_and_fires_once_late() {
    let (mut session, pane) = armed(0, 0, 30, "npm test", true);
    server(&mut session).deactivate(DeactivationReason::Shutdown);
    server(&mut session).activate::<ilium_plugin_core_scheduler::SchedulerServer>();
    let submissions = server(&mut session).deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: format!("send:{}", pane.0),
        late_by_millis: 600_000,
    }));
    let sent = sends(&submissions);
    assert_eq!(sent.len(), 1, "a timer that came due while Ilium was stopped fires once, late");
    assert_eq!(sent[0].0, plain_spec(pane, "npm test", Delivery::Submitted));
}

/// REQ: F-055, scheduler-B10
#[test]
fn arm_055_a_restart_does_not_resend_after_the_timer_fired_once() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    assert_eq!(sent_after(&mut session, 30_000), 1);
    server(&mut session).deactivate(DeactivationReason::Shutdown);
    server(&mut session).activate::<ilium_plugin_core_scheduler::SchedulerServer>();
    let submissions = server(&mut session).deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: format!("send:{}", pane.0),
        late_by_millis: 1,
    }));
    assert!(sends(&submissions).is_empty());
}

/// REQ: F-055, scheduler-B09
#[test]
fn arm_055_the_send_does_not_check_whether_the_agent_is_idle() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).tree().set_activity(pane.0, AgentActivity::Working);
    assert_eq!(sent_after(&mut session, 30_000), 1);
}

/// REQ: F-055, scheduler-B09
#[test]
fn arm_055_the_send_does_not_wait_for_an_approval_prompt_to_clear() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).tree().set_activity(pane.0, AgentActivity::AwaitingApproval);
    assert_eq!(sent_after(&mut session, 30_000), 1);
}

/// REQ: F-055, scheduler-B09
#[test]
fn arm_055_a_pane_that_is_not_ready_is_retried_after_the_workspace_recovers() {
    let (mut session, pane) = armed(0, 0, 30, "x", true);
    server(&mut session).advance_clock(30_000);
    let first = server(&mut session).pending_requests();
    assert_eq!(first.len(), 1);
    let not_ready = PluginError { code: ErrorCode::PaneNotReady, message: "working directory missing".to_string(), retry_after_ms: None };
    server(&mut session).complete(first[0].clone(), Outcome::Refused(not_ready));
    server(&mut session).deliver_one(Event::PaneLifecycleChanged(PaneLifecycleEvent {
        pane,
        state: PaneLifecycleState::Running,
        by: None,
    }));
    server(&mut session).settle();
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent.len(), 2, "one refused attempt and one retry");
    assert_eq!(sent[0].1, sent[1].1, "the retry reuses the dedup key");
}

/// REQ: F-055, scheduler-B09
#[test]
fn arm_055_a_not_ready_pane_is_not_retried_before_recovery() {
    let (mut session, _pane) = armed(0, 0, 30, "x", true);
    server(&mut session).advance_clock(30_000);
    let first = server(&mut session).pending_requests();
    let not_ready = PluginError { code: ErrorCode::PaneNotReady, message: "working directory missing".to_string(), retry_after_ms: None };
    server(&mut session).complete(first[0].clone(), Outcome::Refused(not_ready));
    server(&mut session).settle();
    assert_eq!(sends(&server(&mut session).submitted()).len(), 1, "no retry without the recovery event");
}

/// REQ: F-055, scheduler-B08
#[test]
fn arm_055_a_duplicate_outcome_is_not_sent_again() {
    let (mut session, _pane) = armed(0, 0, 30, "x", true);
    server(&mut session).advance_clock(30_000);
    let first = server(&mut session).pending_requests();
    server(&mut session).complete(first[0].clone(), Outcome::Duplicate(first[0].clone()));
    server(&mut session).settle();
    assert_eq!(sends(&server(&mut session).submitted()).len(), 1);
}

/// REQ: F-055
#[test]
fn arm_055_a_late_by_value_does_not_change_the_text() {
    let (mut session, pane) = armed(0, 0, 30, "keep me", true);
    let submissions = server(&mut session).deliver_one(Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(1),
        name: format!("send:{}", pane.0),
        late_by_millis: 86_400_000,
    }));
    assert_eq!(sends(&submissions)[0].0.text, "keep me");
}
