//! Twin of ilium-server/src/ipc/ordered_owner_regressions.rs.

use crate::common::*;

/// REQ: PORT:ilium-server/src/ipc/ordered_owner_regressions.rs::queued_prompt_attempt_is_persisted_before_success_and_restored_without_replay, scheduler-B19, scheduler-B21
#[test]
fn port_ordered_owner_queued_prompt_attempt_is_persisted_before_success_and_restored_without_replay(
) {
    let mut session = session();
    let pane = terminal_pane(&mut session, "agent");
    enqueue(&mut session, pane, "authored-queue-message", "once", 1).expect("enqueue");
    // The delivery is recorded as attempted before the write; the write then fails to complete.
    server(&mut session).crash_after_journal_write();
    let first = sends(&finish_turn(&mut session, pane, 1));
    assert!(first.len() <= 1);
    let retained = list_queue(&mut session, pane);
    assert_eq!(
        retained.len(),
        1,
        "the head stays in the queue after the attempt"
    );
    assert_eq!(retained[0]["text"], "authored-queue-message");
    assert_eq!(retained[0]["attempted_delivery"], true);
    // A restart restores the same queue and does not replay the attempted head.
    server(&mut session).deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    server(&mut session).activate::<ilium_plugin_core_scheduler::SchedulerServer>();
    assert_eq!(list_queue(&mut session, pane), retained);
    let again = sends(&finish_turn(&mut session, pane, 2));
    assert!(again.is_empty(), "no replay after restore: {again:?}");
    assert_eq!(list_queue(&mut session, pane), retained);
}

/// REQ: PORT:ilium-server/src/ipc/ordered_owner_regressions.rs::queued_prompt_attempt_is_persisted_before_success_and_restored_without_replay
#[test]
fn port_ordered_owner_second_queued_prompt_waits_for_the_next_turn() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "agent");
    enqueue(&mut session, pane, "first-prompt", "once", 1).expect("enqueue first");
    enqueue(&mut session, pane, "second-prompt", "once", 1).expect("enqueue second");
    let first = sends(&finish_turn(&mut session, pane, 1));
    assert_eq!(first.len(), 1, "one prompt per finished turn");
    assert_eq!(first[0].0.text, "first-prompt");
    let rest = list_queue(&mut session, pane);
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0]["text"], "second-prompt");
}
