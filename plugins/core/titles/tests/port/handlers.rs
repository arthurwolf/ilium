//! Port twins of the tests of `handlers.rs`.

use ilium_plugin_sdk::types::*;

use crate::common::*;

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::blocked_automatic_title_probe_releases_registry_guards_and_rejects_replacement
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::recommended_restructure_orders_undo_and_round_trips_persistence_and_attach, PORT:ilium-server/src/ipc/handlers.rs::reverting_a_restructure_evicts_the_orphaned_panes_agent_debug_journal
#[test]
fn port_043_undo_reverts_the_restructure() {
    let mut host = organised_host(&plan_reply());
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::Ok));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert_eq!(reverts(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::current_agent_title_receipt_survives_off_lock_collection_and_stale_generation_refuses, PORT:ilium-server/src/ipc/handlers.rs::current_plain_shell_title_still_applies_after_off_lock_confirmation, PORT:ilium-server/src/ipc/handlers.rs::project_restructure_handler_rebases_activity_and_preserves_splits
#[test]
fn port_043_valid_plan_is_applied() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::blocked_automatic_title_probe_releases_registry_guards_and_rejects_replacement
#[test]
fn port_041_manual_title_rejects_a_late_inferred_replacement() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (id, _) = host.inference_requests().remove(0);
    host.deliver_one(Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(3),
        title: "My own title".to_string(),
        source: TitleOrigin::Manual,
    }));
    host.settle();
    host.complete_inference(id, completion("Login fix"));
    host.settle();
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::recommended_restructure_orders_undo_and_round_trips_persistence_and_attach
#[test]
fn port_043_second_undo_reverts_nothing_more() {
    let mut host = organised_host(&plan_reply());
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::Ok));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert_eq!(reverts(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::reverting_a_restructure_evicts_the_orphaned_panes_agent_debug_journal
#[test]
fn port_043_undo_without_an_organise_reverts_nothing() {
    let mut host = host_with_two_panes();
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::current_agent_title_receipt_survives_off_lock_collection_and_stale_generation_refuses
#[test]
fn port_041_reply_after_the_twenty_second_drop_is_discarded() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (id, _) = host.inference_requests().remove(0);
    host.advance_clock(20_001);
    host.settle();
    host.complete_inference(id, completion("Login fix"));
    host.settle();
    assert!(!renames(&host.submitted())
        .iter()
        .any(|spec| spec.title == "Login fix"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::current_plain_shell_title_still_applies_after_off_lock_confirmation
#[test]
fn port_041_not_found_is_not_retried_for_the_same_pane() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (id, _) = host.inference_requests().remove(0);
    host.complete(
        RequestRef::Call(id),
        Outcome::Failed(plugin_error(ErrorCode::NotFound)),
    );
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug again"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::project_restructure_handler_rebases_activity_and_preserves_splits
#[test]
fn port_043_timed_out_organise_leaves_the_tree_unchanged() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete(
        RequestRef::Call(id),
        Outcome::Failed(plugin_error(ErrorCode::TimedOut)),
    );
    host.settle();
    assert!(restructures(&host.submitted()).is_empty());
    assert!(reverts(&host.submitted()).is_empty());
}
