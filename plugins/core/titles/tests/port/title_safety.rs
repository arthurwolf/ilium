//! Port twins of the tests of `title_safety.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-core/tests/title_safety.rs::ai_restructure_does_not_claim_user_title_ownership, PORT:ilium-core/tests/title_safety.rs::regrouping_keeps_genuine_manual_title_ownership
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
    host.deliver_one(outcome_event(correlation, Outcome::Refused(plugin_error(ErrorCode::Conflict))));
    host.settle();
    assert_eq!(renames(&host.submitted()).len(), 1);
    assert!(host.failures().is_empty());
}
