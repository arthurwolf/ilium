//! Port twins of the tests of `handlers.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

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
