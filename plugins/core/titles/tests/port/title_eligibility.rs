//! Port twins of the tests of `title_eligibility.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-server/src/title_eligibility.rs::ambiguous_non_fixed_user_title_is_not_implicitly_repaired, PORT:ilium-server/src/title_eligibility.rs::fixed_name_and_newer_presentation_win_over_positive_history, PORT:ilium-server/src/title_eligibility.rs::fresh_shell_confirmation_only_refines_the_same_invocation, PORT:ilium-server/src/title_eligibility.rs::missing_and_empty_history_need_exact_authored_delivery_receipt, PORT:ilium-server/src/title_eligibility.rs::non_terminal_presentation_compares_revision_and_fixed_ownership, PORT:ilium-server/src/title_eligibility.rs::over_limit_transcript_batch_fails_closed_without_partial_authorization, PORT:ilium-server/src/title_eligibility.rs::oversized_transcript_line_fails_closed_without_partial_authorization, PORT:ilium-server/src/title_eligibility.rs::positive_own_history_grants_but_all_identity_changes_decline, PORT:ilium-server/src/title_eligibility.rs::receipt_before_session_discovery_binds_the_exact_invocation, PORT:ilium-server/src/title_eligibility.rs::shell_grants_need_current_and_captured_confirmation, PORT:ilium-server/src/title_eligibility.rs::unresolved_terminal_declines_even_with_a_receipt
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
