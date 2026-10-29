//! F-049 rule 2, merge repeats: alerts of the same kind on the same pane closer together than the
//! window collapse into the first. 30 s default, 10 s steps up to 600 s, 0 disables, values above 600
//! are clamped. Successes and failures are counted separately (src/docs/notifications.md; the
//! coalescer of ilium-server/src/notifications.rs).

use proptest::prelude::*;
use serde_json::json;

use crate::common::*;
use crate::feature_only::{notices, sounds, task_outcome};

fn failed_alerts(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    pane: ilium_plugin_sdk::types::NodeId,
    job: &str,
) -> usize {
    let submissions = task_outcome(host, pane, job, "error");
    notices(&submissions).len() + sounds(&submissions).len()
}

/// REQ: F-049
#[test]
fn merge_049_the_first_outcome_in_a_window_is_admitted() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(failed_alerts(&mut host, pane, "job-1"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_repeat_one_second_later_collapses_into_the_first() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(1_000);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 0);
}

/// REQ: F-049
#[test]
fn merge_049_a_repeat_one_millisecond_before_the_default_window_ends_is_dropped() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(29_999);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 0);
}

/// REQ: F-049
#[test]
fn merge_049_a_repeat_exactly_at_the_default_window_is_admitted() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(30_000);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_repeat_one_millisecond_after_the_default_window_is_admitted() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(30_001);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_dropped_repeat_does_not_extend_the_window() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(20_000);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 0);
    host.advance_clock(10_000);
    assert_eq!(
        failed_alerts(&mut host, pane, "job-3"),
        2,
        "30 s after the first admitted alert"
    );
}

/// REQ: F-049
#[test]
fn merge_049_the_window_is_counted_per_pane() {
    let mut host = server_host();
    let first = add_pane(&mut host, "build-one", None);
    let second = add_pane(&mut host, "build-two", None);
    assert_eq!(failed_alerts(&mut host, first, "job-1"), 2);
    assert_eq!(failed_alerts(&mut host, second, "job-2"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_success_and_a_failure_on_one_pane_do_not_merge() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-1", "done")).len(),
        1
    );
    host.advance_clock(1_000);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-2", "error")).len(),
        1
    );
}

/// REQ: F-049
#[test]
fn merge_049_a_failure_is_never_swallowed_by_an_earlier_success() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    task_outcome(&mut host, pane, "job-1", "done");
    host.advance_clock(1);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-2", "lost")).len(),
        1
    );
}

/// REQ: F-049
#[test]
fn merge_049_two_successes_on_one_pane_merge() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-1", "done")).len(),
        1
    );
    host.advance_clock(1_000);
    assert!(notices(&task_outcome(&mut host, pane, "job-2", "done")).is_empty());
}

/// REQ: F-049
#[test]
fn merge_049_a_failed_task_and_a_lost_monitor_count_as_the_same_kind() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-1", "error")).len(),
        1
    );
    host.advance_clock(1_000);
    assert!(notices(&task_outcome(&mut host, pane, "job-2", "lost")).is_empty());
}

/// REQ: F-049
#[test]
fn merge_049_a_zero_window_disables_merging() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(0));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(failed_alerts(&mut host, pane, "job-1"), 2);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 2);
    assert_eq!(failed_alerts(&mut host, pane, "job-3"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_ten_second_window_admits_a_repeat_after_ten_seconds() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(10));
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(9_999);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 0);
    host.advance_clock(1);
    assert_eq!(failed_alerts(&mut host, pane, "job-3"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_window_of_600_seconds_is_kept() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(600));
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(599_999);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 0);
    host.advance_clock(1);
    assert_eq!(failed_alerts(&mut host, pane, "job-3"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_a_window_above_600_seconds_is_clamped_to_600() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(601));
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    host.advance_clock(600_000);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 2);
}

/// REQ: F-049
#[test]
fn merge_049_the_merge_applies_to_sounds_and_notices_alike() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    task_outcome(&mut host, pane, "job-1", "error");
    host.advance_clock(1_000);
    let repeat = task_outcome(&mut host, pane, "job-2", "error");
    assert!(sounds(&repeat).is_empty());
    assert!(notices(&repeat).is_empty());
}

/// REQ: F-049
#[test]
fn merge_049_agent_finished_alerts_are_not_task_alerts_and_never_merge() {
    let mut host = server_host();
    let node = add_pane(
        &mut host,
        "claude-api",
        Some(ilium_plugin_sdk::types::AgentActivity::Working),
    );
    assert_eq!(notices(&finish_turn(&mut host, node, 1)).len(), 1);
    host.advance_clock(1_000);
    let again = transition(
        &mut host,
        node,
        ilium_plugin_sdk::types::AgentActivity::Working,
        ilium_plugin_sdk::types::AgentActivity::Done,
        2,
    );
    assert_eq!(notices(&again).len(), 1);
}

/// REQ: F-049
#[test]
fn merge_049_the_window_can_be_changed_while_the_plugin_runs() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    failed_alerts(&mut host, pane, "job-1");
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(0));
    host.advance_clock(1);
    assert_eq!(failed_alerts(&mut host, pane, "job-2"), 2);
}

proptest! {
    // REQ: F-049
    #[test]
    fn merge_049_property_a_repeat_is_admitted_exactly_when_the_gap_reaches_the_window(
        window_steps in 0u32..=60,
        gap_ms in 0u64..700_000,
    ) {
        let window_seconds = window_steps * 10;
        let mut host = server_host();
        set_setting(&mut host, "notifications.task_coalesce_seconds", json!(window_seconds));
        let pane = add_pane(&mut host, "build-shell", None);
        let first = task_outcome(&mut host, pane, "job-1", "error");
        prop_assert_eq!(notices(&first).len(), 1);
        host.advance_clock(gap_ms);
        let second = task_outcome(&mut host, pane, "job-2", "error");
        let expected = window_seconds == 0 || gap_ms >= u64::from(window_seconds) * 1_000;
        prop_assert_eq!(notices(&second).len() == 1, expected);
    }
}
