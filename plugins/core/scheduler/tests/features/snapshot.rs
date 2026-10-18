//! Storage (scheduler-B21): the snapshot section `ilium.core.scheduler` with `scheduled_input` and `prompt_queue` per pane.

use ilium_plugin_sdk::types::{DeactivationReason, Event, NodeId};
use serde_json::Value;

use crate::common::*;
use crate::feature_support::*;

fn reactivate(session: &mut ilium_plugin_sdk::testing::FakeSession) {
    server(session).deactivate(DeactivationReason::Shutdown);
    server(session).activate::<ilium_plugin_core_scheduler::SchedulerServer>();
}

/// REQ: scheduler-B21, F-055, F-057
#[test]
fn snapshot_b21_the_migration_page_gives_the_exact_section_shape() {
    let page = manual_page("reference-config-and-data-migration.md");
    assert!(page.contains(
        "`plugin_sections[\"ilium.core.scheduler\"] = {\"schema_version\":1,\"data\":{\"panes\":[{\"pane_id\":<n>,\"scheduled_input\":<v3 value or null>,\"prompt_queue\":<v3 value>}]}}`"
    ));
}

/// REQ: scheduler-B21, F-055, F-057
#[test]
fn snapshot_b21_the_v3_pane_fields_move_to_the_scheduler_section() {
    let page = manual_page("reference-config-and-data-migration.md");
    assert!(page.contains("| `tree` pane fields `scheduled_input`, `prompt_queue` | Pending scheduled input and prompt queue | Scheduler section |"));
}

/// REQ: scheduler-B21, F-055
#[test]
fn snapshot_b21_the_kernel_keeps_the_pending_marker_while_the_scheduler_is_disabled() {
    let page = manual_page("reference-kernel-and-boot.md");
    assert!(
        page.contains("The tree still shows \"scheduled input pending\" from the stored section")
    );
    assert!(page.contains("The section `ilium.core.scheduler` stays in the snapshot and fires on return, marked `late`"));
}

/// REQ: scheduler-B21, F-057
#[test]
fn snapshot_b21_the_queue_survives_a_server_side_restart_in_order() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    for (text, mode, times) in [("a", "once", 1), ("b", "times", 3), ("c", "forever", 1)] {
        enqueue(&mut session, pane, text, mode, times).expect("enqueue");
    }
    let before = list_queue(&mut session, pane);
    reactivate(&mut session);
    assert_eq!(list_queue(&mut session, pane), before);
}

/// REQ: scheduler-B21, F-057
#[test]
fn snapshot_b21_a_restored_queue_keeps_delivering_one_per_finished_turn() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "a", "once", 1).expect("enqueue");
    enqueue(&mut session, pane, "b", "once", 1).expect("enqueue");
    reactivate(&mut session);
    let sent = sends(&finish_turn(&mut session, pane, 1));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0.text, "a");
}

/// REQ: scheduler-B21, F-057
#[test]
fn snapshot_b21_a_restored_repeat_count_is_the_decreased_one() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "r", "times", 3).expect("enqueue");
    finish_turn(&mut session, pane, 1);
    reactivate(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["times"], 2);
}

/// REQ: scheduler-B21, F-057
#[test]
fn snapshot_b21_queues_are_stored_per_pane() {
    let mut session = session();
    let first = terminal_pane(&mut session, "one");
    let second = terminal_pane(&mut session, "two");
    enqueue(&mut session, first, "a", "once", 1).expect("enqueue");
    enqueue(&mut session, second, "b", "once", 1).expect("enqueue");
    enqueue(&mut session, second, "c", "once", 1).expect("enqueue");
    reactivate(&mut session);
    assert_eq!(list_queue(&mut session, first).len(), 1);
    assert_eq!(list_queue(&mut session, second).len(), 2);
}

/// REQ: scheduler-B21, F-057, scheduler-B10
#[test]
fn snapshot_b21_closing_the_pane_drops_its_queue() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "a", "once", 1).expect("enqueue");
    server(&mut session).deliver_one(Event::NodeRemoved(pane));
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B21, F-057
#[test]
fn snapshot_b21_a_queue_for_an_unknown_pane_is_refused() {
    let mut session = session();
    // manual does not pin the error code for an unknown pane
    let refused = enqueue(&mut session, NodeId(9999), "a", "once", 1).expect_err("refused");
    assert!(!refused.message.is_empty(), "{refused:?}");
    assert!(list_queue(&mut session, NodeId(9999)).is_empty());
}

/// REQ: scheduler-B21, F-055, F-057
#[test]
fn snapshot_b21_the_session_storage_bucket_is_valid_json_when_the_plugin_stores_in_it() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "a", "once", 1).expect("enqueue");
    let dump = server(&mut session)
        .storage()
        .dump(ilium_plugin_sdk::host::Bucket::Session);
    for (key, bytes) in dump {
        assert!(
            serde_json::from_slice::<Value>(&bytes).is_ok(),
            "{key} is JSON"
        );
    }
}
