//! Port twins of the tests of `title_delivery_live.rs`.

use ilium_plugin_sdk::types::*;

use crate::common::*;

/// REQ: PORT:ilium-server/tests/title_delivery_live.rs::acknowledged_authored_enter_authorizes_title_before_request_history_flush
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}

/// REQ: PORT:ilium-server/tests/title_delivery_live.rs::exited_agent_titles_require_verified_own_history_and_preserve_manual_names
#[test]
fn port_041_refused_rename_is_not_retried() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "work on something", "Fix login");
    let correlation = host
        .submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::RenameNode(_)))
        .expect("rename submission")
        .correlation;
    host.deliver_one(outcome_event(
        correlation,
        Outcome::Refused(plugin_error(ErrorCode::Conflict)),
    ));
    host.settle();
    assert_eq!(renames(&host.submitted()).len(), 1);
    assert!(host.failures().is_empty());
}
