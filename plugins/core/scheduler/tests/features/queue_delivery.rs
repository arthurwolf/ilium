//! Prompt queue delivery on the server (F-057): scheduler-B16 to B20.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{AgentActivity, Delivery, ErrorCode, Event, NodeId};
use proptest::prelude::*;

use crate::common::*;

fn pane_session() -> (FakeSession, NodeId) {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    (session, pane)
}

fn texts(session: &mut FakeSession, pane: NodeId, finishes: u64) -> Vec<String> {
    let mut delivered = Vec::new();
    for revision in 1..=finishes {
        for (spec, _) in sends(&finish_turn(session, pane, revision)) {
            delivered.push(spec.text);
        }
    }
    delivered
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_working_to_idle_delivers_the_head() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "next task", "once", 1).expect("enqueue");
    let sent = sends(&finish_turn(&mut session, pane, 1));
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].0,
        plain_spec(pane, "next task", Delivery::Submitted)
    );
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_working_to_done_delivers_the_head() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "next task", "once", 1).expect("enqueue");
    let submissions = server(&mut session).deliver_one(activity_event(
        pane,
        AgentActivity::Working,
        AgentActivity::Done,
        1,
    ));
    assert_eq!(sends(&submissions).len(), 1);
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_other_transitions_deliver_nothing() {
    use AgentActivity::*;
    let transitions = [
        (Idle, Working),
        (None, Idle),
        (None, Working),
        (AwaitingApproval, Idle),
        (AwaitingApproval, Done),
        (Idle, Done),
        (Done, Idle),
        (Working, AwaitingApproval),
        (Working, Menu),
        (Working, None),
        (Idle, Idle),
    ];
    for (previous, current) in transitions {
        let (mut session, pane) = pane_session();
        enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
        let submissions =
            server(&mut session).deliver_one(activity_event(pane, previous, current, 1));
        assert!(
            sends(&submissions).is_empty(),
            "{previous:?} -> {current:?}"
        );
    }
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_an_idle_agent_alone_does_not_deliver_and_time_does_not_either() {
    let (mut session, pane) = pane_session();
    server(&mut session)
        .tree()
        .set_activity(pane.0, AgentActivity::Idle);
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    server(&mut session).advance_clock(24 * 3_600_000);
    assert!(
        sends(&server(&mut session).submitted()).is_empty(),
        "a queue is not a timer"
    );
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_a_finish_with_an_empty_queue_sends_nothing() {
    let (mut session, pane) = pane_session();
    assert!(sends(&finish_turn(&mut session, pane, 1)).is_empty());
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_one_prompt_per_finished_turn() {
    let (mut session, pane) = pane_session();
    for text in ["a", "b", "c"] {
        enqueue(&mut session, pane, text, "once", 1).expect("enqueue");
    }
    let first = sends(&finish_turn(&mut session, pane, 1));
    assert_eq!(first.len(), 1, "never more than one per finish");
    assert_eq!(list_queue(&mut session, pane).len(), 2);
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_a_finish_on_another_pane_does_not_deliver_here() {
    let (mut session, pane) = pane_session();
    let other = terminal_pane(&mut session, "other");
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    assert!(sends(&finish_turn(&mut session, other, 1)).is_empty());
    assert_eq!(list_queue(&mut session, pane).len(), 1);
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_the_send_is_submitted_text_then_enter_with_a_queue_dedup_key() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    let sent = sends(&finish_turn(&mut session, pane, 7));
    assert_eq!(sent[0].0.delivery, Delivery::Submitted);
    assert_eq!(sent[0].1, Some(format!("queue:{}:7", pane.0)));
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_the_same_revision_twice_carries_the_same_dedup_key() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "a", "forever", 1).expect("enqueue");
    let first = sends(&finish_turn(&mut session, pane, 3));
    let second = sends(&finish_turn(&mut session, pane, 3));
    assert_eq!(
        first[0].1, second[0].1,
        "the kernel dedups the repeat of one revision"
    );
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_the_queue_is_first_in_first_out() {
    let (mut session, pane) = pane_session();
    for text in ["a", "b", "c"] {
        enqueue(&mut session, pane, text, "once", 1).expect("enqueue");
    }
    assert_eq!(texts(&mut session, pane, 4), ["a", "b", "c"]);
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_a_once_entry_is_removed_after_its_delivery() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "a", "once", 1).expect("enqueue");
    finish_turn(&mut session, pane, 1);
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_run_x_times_sends_exactly_x_times() {
    for times in [1u32, 2, 3, 5] {
        let (mut session, pane) = pane_session();
        enqueue(&mut session, pane, "r", "times", times).expect("enqueue");
        let delivered = texts(&mut session, pane, u64::from(times) + 3);
        assert_eq!(delivered.len(), times as usize, "times={times}");
    }
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_a_repeating_entry_goes_to_the_tail_with_its_count_decreased() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "r", "times", 3).expect("enqueue");
    enqueue(&mut session, pane, "o", "once", 1).expect("enqueue");
    finish_turn(&mut session, pane, 1);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries[0]["text"], "o");
    assert_eq!(entries[1]["text"], "r");
    assert_eq!(entries[1]["times"], 2);
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_other_prompts_get_their_turn_between_repeats() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "r", "times", 2).expect("enqueue");
    enqueue(&mut session, pane, "o", "once", 1).expect("enqueue");
    assert_eq!(texts(&mut session, pane, 5), ["r", "o", "r"]);
}

/// REQ: scheduler-B17, F-057
#[test]
fn qdelivery_b17_forever_stays_in_the_queue_and_rotates() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "f", "forever", 1).expect("enqueue");
    enqueue(&mut session, pane, "o", "once", 1).expect("enqueue");
    assert_eq!(texts(&mut session, pane, 6), ["f", "o", "f", "f", "f", "f"]);
    assert_eq!(list_queue(&mut session, pane).len(), 1);
}

proptest! {
    /// REQ: scheduler-B17, scheduler-B16, F-057
    #[test]
    fn qdelivery_b17_prop_once_entries_arrive_in_insertion_order(count in 1usize..20) {
        let (mut session, pane) = pane_session();
        let expected: Vec<String> = (0..count).map(|index| format!("prompt-{index}")).collect();
        for text in &expected {
            enqueue(&mut session, pane, text, "once", 1).expect("enqueue");
        }
        prop_assert_eq!(texts(&mut session, pane, count as u64 + 2), expected);
    }
}

/// REQ: scheduler-B18, F-057
#[test]
fn qdelivery_b18_the_hundredth_entry_is_accepted() {
    let (mut session, pane) = pane_session();
    for index in 0..100 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1)
            .unwrap_or_else(|error| panic!("entry {index}: {error:?}"));
    }
    assert_eq!(list_queue(&mut session, pane).len(), 100);
}

/// REQ: scheduler-B18, F-057
#[test]
fn qdelivery_b18_the_101st_entry_is_refused_with_quota_exceeded() {
    let (mut session, pane) = pane_session();
    for index in 0..100 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1).expect("within the limit");
    }
    let error =
        enqueue(&mut session, pane, "one too many", "once", 1).expect_err("the queue is full");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert_eq!(list_queue(&mut session, pane).len(), 100);
}

/// REQ: scheduler-B18, F-057
#[test]
fn qdelivery_b18_the_99th_entry_is_accepted_and_leaves_room_for_one_more() {
    let (mut session, pane) = pane_session();
    for index in 0..99 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1).expect("within the limit");
    }
    assert!(enqueue(&mut session, pane, "last", "once", 1).is_ok());
}

/// REQ: scheduler-B18, F-057
#[test]
fn qdelivery_b18_the_limit_is_per_pane() {
    let (mut session, pane) = pane_session();
    let other = terminal_pane(&mut session, "other");
    for index in 0..100 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1).expect("fill");
    }
    assert!(enqueue(&mut session, other, "fits", "once", 1).is_ok());
}

/// REQ: scheduler-B18, F-057
#[test]
fn qdelivery_b18_a_full_queue_accepts_again_after_a_delivery() {
    let (mut session, pane) = pane_session();
    for index in 0..100 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1).expect("fill");
    }
    finish_turn(&mut session, pane, 1);
    assert!(enqueue(&mut session, pane, "again", "once", 1).is_ok());
}

/// REQ: scheduler-B18, scheduler-B17, F-057
#[test]
fn qdelivery_b18_a_repeating_head_of_a_full_queue_is_not_lost_by_the_rotation() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "rep", "forever", 1).expect("head");
    for index in 0..99 {
        enqueue(&mut session, pane, &format!("p{index}"), "once", 1).expect("fill");
    }
    finish_turn(&mut session, pane, 1);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[99]["text"], "rep");
}

/// REQ: scheduler-B19, F-057
#[test]
fn qdelivery_b19_a_crash_during_delivery_does_not_replay_the_head() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "authored-queue-message", "once", 1).expect("enqueue");
    server(&mut session).crash_after_journal_write();
    finish_turn(&mut session, pane, 1);
    assert_eq!(
        server(&mut session).failures().len(),
        1,
        "the injected crash is recorded"
    );
    server(&mut session).advance_clock(60_000);
    let again = sends(&finish_turn(&mut session, pane, 2));
    assert!(
        again.is_empty(),
        "the head may have reached the PTY and is not replayed: {again:?}"
    );
}

/// REQ: scheduler-B19, F-057
#[test]
fn qdelivery_b19_the_head_stays_visible_after_the_crash_marked_as_attempted() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "authored-queue-message", "once", 1).expect("enqueue");
    server(&mut session).crash_after_journal_write();
    finish_turn(&mut session, pane, 1);
    server(&mut session).advance_clock(60_000);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "authored-queue-message");
    assert_eq!(entries[0]["attempted_delivery"], true);
}

/// REQ: scheduler-B19, F-057
#[test]
fn qdelivery_b19_without_a_crash_the_delivered_head_is_gone() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    finish_turn(&mut session, pane, 1);
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B19, F-057
#[test]
fn qdelivery_b19_a_clear_removes_an_attempted_head() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "authored-queue-message", "once", 1).expect("enqueue");
    server(&mut session).crash_after_journal_write();
    finish_turn(&mut session, pane, 1);
    server(&mut session).advance_clock(60_000);
    invoke_menu(&mut session, CLEAR_COMMAND, pane);
    pump(&mut session);
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B20, F-057
#[test]
fn qdelivery_b20_every_queue_delivery_is_journaled() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "a", "forever", 1).expect("enqueue");
    for revision in 1..=3 {
        let sent = sends(&finish_turn(&mut session, pane, revision));
        assert!(sent[0].0.journal, "revision {revision}");
    }
}

/// REQ: scheduler-B20, F-057
#[test]
fn qdelivery_b20_a_queue_delivery_is_told_apart_from_a_scheduled_send_by_its_key() {
    let (mut session, pane) = pane_session();
    server(&mut session).set_clock(1_000_000_000);
    enqueue(&mut session, pane, "q", "once", 1).expect("enqueue");
    schedule(&mut session, pane, 0, 0, 5, "s", true).expect("schedule");
    let queued = sends(&finish_turn(&mut session, pane, 1));
    server(&mut session).advance_clock(5_000);
    let scheduled: Vec<_> = sends(&server(&mut session).submitted())
        .into_iter()
        .filter(|(spec, _)| spec.text == "s")
        .collect();
    assert!(queued[0].1.as_deref().unwrap_or("").starts_with("queue:"));
    assert!(scheduled[0].1.as_deref().unwrap_or("").starts_with("send:"));
}

/// REQ: scheduler-B20, F-057
#[test]
fn qdelivery_b20_the_journal_entry_of_a_delivery_names_the_pane() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "authored-queue-message", "once", 1).expect("enqueue");
    finish_turn(&mut session, pane, 1);
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "journal", "pane": pane.0}),
    )
    .expect("journal answers");
    let entries = reply["entries"].as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "queued-prompt");
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_a_removed_pane_loses_no_prompt_to_a_stale_event() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    server(&mut session).deliver_one(Event::NodeRemoved(pane));
    assert!(sends(&finish_turn(&mut session, pane, 1)).is_empty());
}

/// REQ: scheduler-B16, F-057
#[test]
fn qdelivery_b16_the_queue_is_delivered_with_no_client_attached() {
    let (mut session, pane) = pane_session();
    enqueue(&mut session, pane, "p", "once", 1).expect("enqueue");
    let sent = sends(&finish_turn(&mut session, pane, 1));
    assert_eq!(sent.len(), 1, "the server side needs no client side");
}
